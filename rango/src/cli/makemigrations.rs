use anyhow::{bail, Context, Result};
use rango_core::{ColumnDef, ColumnType, Constraint, DefaultValue, TableSchema};
use std::fs;

use crate::scanner::{scan_models, M2MRelation};
use crate::snapshot::{diff, Snapshot, SchemaDiff};

pub fn run(src_dir: &str, output_dir: &str, prefix: Option<&str>, dry_run: bool, check: bool) -> Result<()> {
    println!("🔍 Scanning models in {}...", src_dir);

    let prefix = match prefix {
        Some(p) => p.to_string(),
        None => detect_project_name()?,
    };
    // Normalize prefix: hyphens → underscores (Cargo names use hyphens, SQL identifiers don't)
    let prefix = prefix.replace('-', "_");
    println!("📦 Table prefix: {}", prefix);

    let (mut schemas, m2m_relations) = scan_models(src_dir)?;

    // Drop unmanaged models — they are not owned by Rango migrations
    schemas.retain(|s| s.managed);

    if schemas.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    // Apply prefix to model tables
    for schema in &mut schemas {
        schema.table_name = format!("{}_{}", prefix, schema.table_name);
    }

    // Generate M2M pivot tables (also prefixed)
    let pivot_schemas: Vec<TableSchema> = m2m_relations.iter()
        .map(|rel| generate_pivot_table(rel, &prefix))
        .collect();

    println!("Found {} model(s):", schemas.len());
    for s in &schemas {
        println!("  - {} ({} columns)", s.table_name, s.columns.len());
    }
    if !pivot_schemas.is_empty() {
        println!("Found {} M2M relation(s):", pivot_schemas.len());
        for s in &pivot_schemas {
            println!("  - {} (pivot)", s.table_name);
        }
    }

    // Merge pivot tables into schemas for diff
    schemas.extend(pivot_schemas);

    // Load previous snapshot
    let old_snapshot = Snapshot::load(output_dir)?;

    // Compute diff
    let diffs = diff(&old_snapshot, &schemas);

    if diffs.is_empty() {
        println!("✅ Nothing to migrate — models match the snapshot.");
        return Ok(());
    }

    // Generate SQL from diff
    let sql = generate_sql_from_diff(&diffs);

    // --dry-run: print SQL, do not write files
    if dry_run {
        println!("\n--- SQL (dry run) ---\n{}\n--- end ---", sql);
        return Ok(());
    }

    // --check: exit 1 if migrations are pending (CI guard)
    if check {
        let next_num = next_migration_number(output_dir)?;
        let label = migration_label(&diffs);
        eprintln!("❌ Pending migration detected: {:04}_{}.sql", next_num, label);
        eprintln!("   Run `rango makemigrations` to generate it.");
        std::process::exit(1);
    }

    fs::create_dir_all(output_dir)?;

    let next_num = next_migration_number(output_dir)?;
    let label = migration_label(&diffs);
    let filename = format!("{}/{:04}_{}.sql", output_dir, next_num, label);
    fs::write(&filename, &sql)?;
    println!("✅ Migration generated: {}", filename);

    // Save new snapshot
    let new_snapshot = Snapshot::from_schemas(&schemas);
    new_snapshot.save(output_dir)?;
    println!("📸 Snapshot updated.");

    Ok(())
}

fn generate_sql_from_diff(diffs: &[SchemaDiff]) -> String {
    let mut sql = String::new();
    for d in diffs {
        match d {
            SchemaDiff::CreateTable(schema) => {
                sql.push_str(&generate_create_table(schema));
                sql.push('\n');
            }
            SchemaDiff::DropTable(name) => {
                sql.push_str(&format!("DROP TABLE IF EXISTS \"{}\";\n\n", name));
            }
            SchemaDiff::AddColumn { table, column } => {
                let def = column_definition(column);
                sql.push_str(&format!(
                    "ALTER TABLE \"{}\" ADD COLUMN {};\n\n", table, def
                ));
            }
            SchemaDiff::DropColumn { table, column } => {
                sql.push_str(&format!(
                    "ALTER TABLE \"{}\" DROP COLUMN \"{}\";\n\n", table, column
                ));
            }
            SchemaDiff::AlterColumnType { table, column, new_type } => {
                sql.push_str(&format!(
                    "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" TYPE {};\n\n",
                    table, column, sql_type(new_type)
                ));
            }
            SchemaDiff::AlterColumnNullable { table, column, nullable } => {
                let op = if *nullable { "DROP NOT NULL" } else { "SET NOT NULL" };
                sql.push_str(&format!(
                    "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" {};\n\n",
                    table, column, op
                ));
            }
            SchemaDiff::AlterColumnUnique { table, column, unique } => {
                if *unique {
                    sql.push_str(&format!(
                        "ALTER TABLE \"{}\" ADD CONSTRAINT \"{}_{}_unique\" UNIQUE (\"{}\");\n\n",
                        table, table, column, column
                    ));
                } else {
                    sql.push_str(&format!(
                        "ALTER TABLE \"{}\" DROP CONSTRAINT IF EXISTS \"{}_{}_unique\";\n\n",
                        table, table, column
                    ));
                }
            }
        }
    }
    sql
}

fn generate_create_table(schema: &TableSchema) -> String {
    let mut lines = Vec::new();
    let mut post_stmts: Vec<String> = Vec::new();

    for col in &schema.columns {
        lines.push(format!("    {}", column_definition(col)));
    }

    // Detect pivot table: 2 UUID NOT NULL non-PK columns → composite PK
    let is_pivot = schema.columns.len() == 2
        && schema.columns.iter().all(|c| !c.primary_key && !c.nullable
            && c.col_type == rango_core::ColumnType::Uuid);

    if is_pivot {
        let col1 = &schema.columns[0].name;
        let col2 = &schema.columns[1].name;
        lines.push(format!("    PRIMARY KEY (\"{}\", \"{}\")", col1, col2));
    }

    // Table-level constraints
    for constraint in &schema.constraints {
        match constraint {
            Constraint::Check(cc) => {
                let name = if cc.name.is_empty() {
                    format!("{}_check", schema.table_name)
                } else {
                    cc.name.clone()
                };
                lines.push(format!("    CONSTRAINT \"{}\" CHECK ({})", name, cc.sql));
            }
            Constraint::Unique(uc) if uc.condition.is_none() => {
                let cols = uc.fields.iter()
                    .map(|f| format!("\"{}\"", f))
                    .collect::<Vec<_>>()
                    .join(", ");
                let name = if uc.name.is_empty() {
                    format!("{}_{}_unique", schema.table_name, uc.fields.join("_"))
                } else {
                    uc.name.clone()
                };
                lines.push(format!("    CONSTRAINT \"{}\" UNIQUE ({})", name, cols));
            }
            Constraint::Unique(uc) => {
                // Partial unique constraint → separate CREATE UNIQUE INDEX (PostgreSQL)
                let cols = uc.fields.iter()
                    .map(|f| format!("\"{}\"", f))
                    .collect::<Vec<_>>()
                    .join(", ");
                let name = if uc.name.is_empty() {
                    format!("{}_{}_unique", schema.table_name, uc.fields.join("_"))
                } else {
                    uc.name.clone()
                };
                let cond = uc.condition.as_deref().unwrap_or("");
                post_stmts.push(format!(
                    "CREATE UNIQUE INDEX \"{}\" ON \"{}\" ({}) WHERE {};\n",
                    name, schema.table_name, cols, cond
                ));
            }
        }
    }

    let mut sql = format!(
        "CREATE TABLE IF NOT EXISTS \"{}\" (\n{}\n);\n",
        schema.table_name,
        lines.join(",\n")
    );

    for stmt in post_stmts {
        sql.push('\n');
        sql.push_str(&stmt);
    }

    sql
}

fn column_definition(col: &ColumnDef) -> String {
    let mut def = format!("\"{}\" {}", col.name, sql_type(&col.col_type));
    if col.primary_key {
        def.push_str(" PRIMARY KEY");
    } else {
        if !col.nullable {
            def.push_str(" NOT NULL");
        }
        if col.unique {
            def.push_str(" UNIQUE");
        }
    }
    if let Some(default) = &col.default {
        match default {
            DefaultValue::CurrentTimestamp => def.push_str(" DEFAULT now()"),
            DefaultValue::Literal(v)       => def.push_str(&format!(" DEFAULT {}", v)),
            DefaultValue::GeneratedUuid    => def.push_str(" DEFAULT gen_random_uuid()"),
        }
    }
    def
}

fn sql_type(col_type: &ColumnType) -> &'static str {
    match col_type {
        ColumnType::Bool           => "BOOLEAN",
        ColumnType::SmallInt       => "SMALLINT",
        ColumnType::Int            => "INTEGER",
        ColumnType::BigInt         => "BIGINT",
        ColumnType::Float          => "REAL",
        ColumnType::Double         => "DOUBLE PRECISION",
        ColumnType::Text           => "TEXT",
        ColumnType::Bytea          => "BYTEA",
        ColumnType::Uuid           => "UUID",
        ColumnType::Date           => "DATE",
        ColumnType::Time           => "TIME",
        ColumnType::DateTime       => "TIMESTAMPTZ",
        ColumnType::Json           => "JSON",
        ColumnType::Jsonb          => "JSONB",
        ColumnType::Varchar(_)     => "VARCHAR",
        ColumnType::Decimal { .. } => "NUMERIC",
    }
}

fn migration_label(diffs: &[SchemaDiff]) -> String {
    let mut tables: Vec<String> = diffs.iter().map(|d| match d {
        SchemaDiff::CreateTable(s)              => s.table_name.clone(),
        SchemaDiff::DropTable(t)                => t.clone(),
        SchemaDiff::AddColumn { table, .. }     => table.clone(),
        SchemaDiff::DropColumn { table, .. }    => table.clone(),
        SchemaDiff::AlterColumnType { table, .. }     => table.clone(),
        SchemaDiff::AlterColumnNullable { table, .. } => table.clone(),
        SchemaDiff::AlterColumnUnique { table, .. }   => table.clone(),
    }).collect::<std::collections::HashSet<_>>().into_iter().collect();
    tables.sort();
    let joined = tables.join("_");
    if joined.len() <= 40 { joined } else { "auto".to_string() }
}

fn next_migration_number(dir: &str) -> Result<u32> {
    let mut max = 0u32;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(num_str) = name.split('_').next() {
                if let Ok(n) = num_str.parse::<u32>() {
                    max = max.max(n);
                }
            }
        }
    }
    Ok(max + 1)
}

/// Generate a pivot table schema for a M2M relation.
fn generate_pivot_table(rel: &M2MRelation, prefix: &str) -> TableSchema {
    let from_table = format!("{}_{}", prefix, rel.from_table);
    let to_table = format!("{}_{}", prefix, rel.to_table);
    let pivot_name = format!("{}_{}", from_table, to_table);

    TableSchema {
        table_name: pivot_name,
        columns: vec![
            rango_core::ColumnDef {
                name: format!("{}_id", from_table),
                col_type: rango_core::ColumnType::Uuid,
                nullable: false,
                primary_key: false,
                unique: false,
                default: None,
                references: None,
            },
            rango_core::ColumnDef {
                name: format!("{}_id", to_table),
                col_type: rango_core::ColumnType::Uuid,
                nullable: false,
                primary_key: false,
                unique: false,
                default: None,
                references: None,
            },
        ],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{generate_sql_from_diff, migration_label};
    use rango_core::{ColumnDef, ColumnType, TableSchema};
    use crate::snapshot::SchemaDiff;

    fn uuid_col(name: &str, pk: bool) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            col_type: ColumnType::Uuid,
            nullable: false,
            primary_key: pk,
            unique: pk,
            default: None,
            references: None,
        }
    }

    fn text_col(name: &str) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            col_type: ColumnType::Text,
            nullable: false,
            primary_key: false,
            unique: false,
            default: None,
            references: None,
        }
    }

    fn simple_table(name: &str) -> TableSchema {
        TableSchema {
            table_name: name.to_string(),
            columns: vec![uuid_col("id", true), text_col("name")],
            ..Default::default()
        }
    }

    // ── CreateTable ────────────────────────────────────────────────────────────

    #[test]
    fn test_create_table_sql() {
        let diff = SchemaDiff::CreateTable(simple_table("users"));
        let sql = generate_sql_from_diff(&[diff]);
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS"), "got: {}", sql);
        assert!(sql.contains("\"users\""), "got: {}", sql);
    }

    #[test]
    fn test_create_table_has_columns() {
        let diff = SchemaDiff::CreateTable(simple_table("orders"));
        let sql = generate_sql_from_diff(&[diff]);
        assert!(sql.contains("\"id\""), "got: {}", sql);
        assert!(sql.contains("\"name\""), "got: {}", sql);
    }

    // ── AddColumn ─────────────────────────────────────────────────────────────

    #[test]
    fn test_add_column_sql() {
        let diff = SchemaDiff::AddColumn {
            table: "x".to_string(),
            column: text_col("title"),
        };
        let sql = generate_sql_from_diff(&[diff]);
        assert!(sql.contains("ALTER TABLE \"x\" ADD COLUMN"), "got: {}", sql);
        assert!(sql.contains("\"title\""), "got: {}", sql);
    }

    // ── DropColumn ────────────────────────────────────────────────────────────

    #[test]
    fn test_drop_column_sql() {
        let diff = SchemaDiff::DropColumn {
            table: "x".to_string(),
            column: "old_col".to_string(),
        };
        let sql = generate_sql_from_diff(&[diff]);
        assert!(sql.contains("ALTER TABLE \"x\" DROP COLUMN"), "got: {}", sql);
        assert!(sql.contains("\"old_col\""), "got: {}", sql);
    }

    // ── AlterColumnNullable ───────────────────────────────────────────────────

    #[test]
    fn test_alter_nullable_true() {
        let diff = SchemaDiff::AlterColumnNullable {
            table: "t".to_string(),
            column: "c".to_string(),
            nullable: true,
        };
        let sql = generate_sql_from_diff(&[diff]);
        assert!(sql.contains("DROP NOT NULL"), "got: {}", sql);
    }

    #[test]
    fn test_alter_nullable_false() {
        let diff = SchemaDiff::AlterColumnNullable {
            table: "t".to_string(),
            column: "c".to_string(),
            nullable: false,
        };
        let sql = generate_sql_from_diff(&[diff]);
        assert!(sql.contains("SET NOT NULL"), "got: {}", sql);
    }

    // ── DropTable ─────────────────────────────────────────────────────────────

    #[test]
    fn test_drop_table_sql() {
        let diff = SchemaDiff::DropTable("old_table".to_string());
        let sql = generate_sql_from_diff(&[diff]);
        assert!(sql.contains("DROP TABLE IF EXISTS"), "got: {}", sql);
        assert!(sql.contains("\"old_table\""), "got: {}", sql);
    }

    // ── Empty diff ────────────────────────────────────────────────────────────

    #[test]
    fn test_empty_diff_produces_empty_string() {
        let sql = generate_sql_from_diff(&[]);
        assert!(sql.is_empty(), "empty diff should produce empty string; got: {:?}", sql);
    }

    // ── migration_label ───────────────────────────────────────────────────────

    #[test]
    fn test_migration_label_single_table() {
        let diffs = vec![SchemaDiff::CreateTable(simple_table("users"))];
        assert_eq!(migration_label(&diffs), "users");
    }

    #[test]
    fn test_migration_label_multiple_tables_sorted() {
        let diffs = vec![
            SchemaDiff::CreateTable(simple_table("posts")),
            SchemaDiff::CreateTable(simple_table("articles")),
        ];
        // tables are deduplicated, sorted, and joined
        assert_eq!(migration_label(&diffs), "articles_posts");
    }

    #[test]
    fn test_migration_label_long_falls_back_to_auto() {
        let diffs: Vec<SchemaDiff> = (0..10)
            .map(|i| SchemaDiff::DropTable(format!("very_long_table_name_{:02}", i)))
            .collect();
        assert_eq!(migration_label(&diffs), "auto");
    }
}

fn detect_project_name() -> Result<String> {
    let cargo_toml = fs::read_to_string("Cargo.toml")
        .context("Could not find Cargo.toml — run from project root or use --prefix")?;
    for line in cargo_toml.lines() {
        let line = line.trim();
        if line.starts_with("name") {
            if let Some(val) = line.splitn(2, '=').nth(1) {
                return Ok(val.trim().trim_matches('"').to_string());
            }
        }
    }
    bail!("Could not detect project name from Cargo.toml — use --prefix")
}
