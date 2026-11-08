use crate::migration::diff::{SchemaDiff, diff, sqlite_rebuilds};
use crate::migration::renames::detect_renames;
use crate::migration::scanner::{generate_pivot_table, scan_models};
use crate::migration::snapshot::Snapshot;
use crate::migration::sql::generate_sql_from_diff;
use anyhow::{Result, bail};
use rango_orm_core::{BackendKind, ColumnType, TableSchema};
use std::fs;
use std::io::{IsTerminal, Write};

/// MySQL rejects table/column names over 64 bytes. Constraint and index names are
/// shortened by the SQL generator, but table names are also derived at runtime
/// (M2M pivots), so they must fit as-is.
/// Pivot columns are UUIDs, so both sides of a ManyToMany need a UUID primary key.
fn check_pivot_targets(schemas: &[TableSchema], pivots: &[TableSchema]) -> Result<()> {
    for pivot in pivots {
        for fk in pivot.columns.iter().filter_map(|c| c.references.as_ref()) {
            let pk_type = schemas
                .iter()
                .find(|s| s.table_name == fk.table)
                .and_then(|s| s.columns.iter().find(|c| c.primary_key))
                .map(|c| &c.col_type);
            if let Some(t) = pk_type.filter(|t| **t != ColumnType::Uuid) {
                bail!(
                    "ManyToMany pivot \"{}\" references \"{}\", whose primary key is {:?}; \
                     ManyToMany needs UUID primary keys on both models",
                    pivot.table_name,
                    fk.table,
                    t
                );
            }
        }
    }
    Ok(())
}

fn check_mysql_identifiers(schemas: &[TableSchema]) -> Result<()> {
    const MAX: usize = 64;
    for s in schemas {
        if s.table_name.len() > MAX {
            bail!(
                "table name `{}` is {} bytes, MySQL allows {}: use a shorter `#[model(table = ...)]` or model name",
                s.table_name,
                s.table_name.len(),
                MAX
            );
        }
        if let Some(c) = s.columns.iter().find(|c| c.name.len() > MAX) {
            bail!(
                "column `{}.{}` is {} bytes, MySQL allows {}",
                s.table_name,
                c.name,
                c.name.len(),
                MAX
            );
        }
    }
    Ok(())
}

pub fn run(
    src_dir: &str,
    output_dir: &str,
    dry_run: bool,
    check: bool,
    assume_yes: bool,
) -> Result<()> {
    let confirm = |question: &str| assume_yes || prompt_yes(question);

    // Detect backend from rango.toml
    let cfg = crate::config::RangoConfig::load().unwrap_or_default();
    let backend = cfg.database.backend_kind();
    let backend_label = match backend {
        BackendKind::Sqlite => "sqlite",
        BackendKind::Mysql => "mysql",
        _ => "postgres",
    };
    println!(
        "🔍 Scanning models in {}... (backend: {})",
        src_dir, backend_label
    );

    let (mut schemas, m2m_relations) = scan_models(src_dir)?;

    // Drop unmanaged models — they are not owned by Rango migrations
    schemas.retain(|s| s.managed);

    if schemas.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    let pivot_schemas: Vec<TableSchema> = m2m_relations.iter().map(generate_pivot_table).collect();

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

    check_pivot_targets(&schemas, &pivot_schemas)?;

    // Merge pivot tables into schemas for diff
    schemas.extend(pivot_schemas);

    if backend == BackendKind::Mysql {
        check_mysql_identifiers(&schemas)?;
    }

    // Load previous snapshot
    let old_snapshot = Snapshot::load(output_dir)?;

    // Compute diff
    let mut diffs = diff(&old_snapshot, &schemas);

    if diffs.is_empty() {
        println!("✅ Nothing to migrate — models match the snapshot.");
        return Ok(());
    }

    // Detect renames, Django-style.
    if check {
        diffs = detect_renames(diffs, &old_snapshot, |_, _, _| false);
    } else {
        diffs = detect_renames(diffs, &old_snapshot, |table, from, to| {
            confirm(&format!(
                "Did you rename \"{}\".\"{}\" to \"{}\"?",
                table, from, to
            ))
        });
    }

    check_not_null_additions(&diffs)?;

    // Generate SQL from diff
    let destructive = destructive_items(&diffs);
    if !destructive.is_empty() {
        eprintln!("⚠️  Destructive changes: {}", destructive.join(", "));
    }
    // SQLite cannot alter columns/constraints in place: those tables are rebuilt.
    let sql_diffs = if backend == BackendKind::Sqlite {
        sqlite_rebuilds(diffs.clone(), &schemas)
    } else {
        diffs.clone()
    };
    let sql = with_destructive_warning(generate_sql_from_diff(&sql_diffs, backend), &destructive);

    // --dry-run: print SQL, do not write files
    if dry_run {
        println!("\n--- SQL (dry run) ---\n{}\n--- end ---", sql);
        return Ok(());
    }

    // --check: exit 1 if migrations are pending (CI guard)
    if check {
        let next_num = next_migration_number(output_dir)?;
        let label = migration_label(&diffs);
        eprintln!(
            "❌ Pending migration detected: {:04}_{}.sql",
            next_num, label
        );
        eprintln!("   Run `rango makemigrations` to generate it.");
        std::process::exit(1);
    }

    if !destructive.is_empty() && !confirm("Continue") {
        bail!("Aborted: destructive changes need confirmation (interactive, or --yes).");
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

fn check_not_null_additions(diffs: &[SchemaDiff]) -> Result<()> {
    for d in diffs {
        if let SchemaDiff::AddColumn { table, column } = d
            && !column.nullable
            && column.default.is_none()
            && !column.primary_key
        {
            let dropped: Vec<&str> = diffs
                .iter()
                .filter_map(|d| match d {
                    SchemaDiff::DropColumn { table: t, column } if t == table => {
                        Some(column.as_str())
                    }
                    _ => None,
                })
                .collect();
            if !dropped.is_empty() {
                bail!(
                    "Column \"{table}\".\"{}\" was added while {} dropped: this may be a rename \
                     of \"{}\", which needs confirmation. Run `rango makemigrations` in a terminal and \
                     answer `y`, or pass `--yes` to confirm every rename and drop non-interactively \
                     (review the SQL with `--dry-run`). Otherwise add #[field(default = \"...\")] or make it Option<T>.",
                    column.name,
                    if dropped.len() == 1 {
                        "another column was"
                    } else {
                        "other columns were"
                    },
                    dropped.join("\", \""),
                );
            }
            bail!(
                "Cannot add NOT NULL column \"{}\".\"{}\" without a default: existing rows would violate it. Add #[field(default = \"...\")] or make it Option<T>.",
                table,
                column.name
            );
        }
    }
    Ok(())
}

fn destructive_items(diffs: &[SchemaDiff]) -> Vec<String> {
    let mut items = Vec::new();
    for d in diffs {
        match d {
            SchemaDiff::DropTable(table) => items.push(format!("table \"{}\"", table)),
            SchemaDiff::DropColumn { table, column } => {
                items.push(format!("column \"{}\".\"{}\"", table, column))
            }
            _ => {}
        }
    }
    items
}

fn with_destructive_warning(sql: String, items: &[String]) -> String {
    if items.is_empty() {
        sql
    } else {
        format!(
            "-- WARNING: this migration drops data: {}\n{}",
            items.join(", "),
            sql
        )
    }
}

fn prompt_yes(prompt: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }

    print!("{} [y/N] ", prompt);
    let _ = std::io::stdout().flush();

    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }

    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

fn migration_label(diffs: &[SchemaDiff]) -> String {
    let mut tables: Vec<String> = diffs
        .iter()
        .map(|d| match d {
            SchemaDiff::CreateTable(s) => s.table_name.clone(),
            SchemaDiff::DropTable(t) => t.clone(),
            SchemaDiff::AddColumn { table, .. } => table.clone(),
            SchemaDiff::DropColumn { table, .. } => table.clone(),
            SchemaDiff::AlterColumnType { table, .. } => table.clone(),
            SchemaDiff::AlterColumnNullable { table, .. } => table.clone(),
            SchemaDiff::AlterColumnUnique { table, .. } => table.clone(),
            SchemaDiff::AlterColumnIndex { table, .. } => table.clone(),
            SchemaDiff::AlterColumnReference { table, .. } => table.clone(),
            SchemaDiff::AddConstraint { table, .. } => table.clone(),
            SchemaDiff::DropConstraint { table, .. } => table.clone(),
            SchemaDiff::RenameColumn { table, .. } => table.clone(),
            SchemaDiff::RebuildTable { schema, .. } => schema.table_name.clone(),
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    tables.sort();
    let joined = tables.join("_");
    if joined.len() <= 40 {
        joined
    } else {
        "auto".to_string()
    }
}

fn next_migration_number(dir: &str) -> Result<u32> {
    let mut max = 0u32;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(num_str) = name.split('_').next()
                && let Ok(n) = num_str.parse::<u32>()
            {
                max = max.max(n);
            }
        }
    }
    Ok(max + 1)
}

#[cfg(test)]
mod tests {
    use super::{check_mysql_identifiers, check_not_null_additions, migration_label};
    use crate::migration::diff::SchemaDiff;
    use rango_orm_core::{ColumnDef, ColumnType, TableSchema};

    fn uuid_col(name: &str, pk: bool) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            col_type: ColumnType::Uuid,
            nullable: false,
            primary_key: pk,
            unique: pk,
            default: None,
            references: None,
            index: false,
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
            index: false,
        }
    }

    fn simple_table(name: &str) -> TableSchema {
        TableSchema {
            table_name: name.to_string(),
            columns: vec![uuid_col("id", true), text_col("name")],
            ..Default::default()
        }
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

    // ── NOT NULL additions ────────────────────────────────────────────────────
    #[test]
    fn test_not_null_add_column_without_default_is_error() {
        let diffs = vec![SchemaDiff::AddColumn {
            table: "t".to_string(),
            column: text_col("title"),
        }];
        assert!(check_not_null_additions(&diffs).is_err());
    }

    #[test]
    fn unconfirmed_rename_error_mentions_rename_and_yes() {
        let diffs = vec![
            SchemaDiff::DropColumn {
                table: "t".to_string(),
                column: "name".to_string(),
            },
            SchemaDiff::AddColumn {
                table: "t".to_string(),
                column: text_col("title"),
            },
        ];
        let msg = check_not_null_additions(&diffs).unwrap_err().to_string();
        assert!(msg.contains("may be a rename of \"name\""), "{msg}");
        assert!(msg.contains("--yes"), "{msg}");
    }

    #[test]
    fn test_not_null_add_column_with_default_is_ok() {
        let mut col = text_col("title");
        col.default = Some(rango_orm_core::DefaultValue::Literal("x".to_string()));
        let diffs = vec![SchemaDiff::AddColumn {
            table: "t".to_string(),
            column: col,
        }];
        assert!(check_not_null_additions(&diffs).is_ok());
    }

    #[test]
    fn mysql_rejects_table_names_over_64_bytes() {
        let ok = simple_table(&"t".repeat(64));
        assert!(check_mysql_identifiers(&[ok]).is_ok());
        let pivot = simple_table(
            "library_management_system_publication_library_management_system_contributor",
        );
        let err = check_mysql_identifiers(&[pivot]).unwrap_err().to_string();
        assert!(err.contains("shorter `#[model(table"), "{err}");
    }
}
