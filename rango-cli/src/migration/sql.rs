use crate::migration::diff::SchemaDiff;
use rango_core::{
    BackendKind, ColumnDef, ColumnType, Constraint, DefaultValue, ReferentialAction, TableSchema,
    constraints::constraint_name, quote_ident,
};

pub(crate) fn generate_sql_from_diff(diffs: &[SchemaDiff], backend: BackendKind) -> String {
    let mut creates: Vec<TableSchema> = Vec::new();
    let mut others: Vec<SchemaDiff> = Vec::new();
    let mut drops: Vec<SchemaDiff> = Vec::new();

    for d in diffs {
        match d {
            SchemaDiff::CreateTable(schema) => creates.push(schema.clone()),
            SchemaDiff::DropTable(_) => drops.push(d.clone()),
            _ => others.push(d.clone()),
        }
    }

    let create_order = sort_create_tables_topologically(&creates);
    let mut sql = String::new();

    for i in create_order {
        sql.push_str(&generate_create_table(&creates[i], backend));
        sql.push('\n');
    }

    for d in &others {
        sql.push_str(&diff_to_sql(d, backend));
    }

    for d in &drops {
        sql.push_str(&diff_to_sql(d, backend));
    }

    sql
}

fn diff_to_sql(d: &SchemaDiff, backend: BackendKind) -> String {
    match d {
        SchemaDiff::CreateTable(schema) => {
            format!("{}\n", generate_create_table(schema, backend))
        }
        SchemaDiff::DropTable(name) => format!("DROP TABLE IF EXISTS {};\n\n", quote_ident(name)),
        SchemaDiff::AddColumn { table, column } => {
            let def = column_definition(table, column, backend);
            let mut out = format!("ALTER TABLE {} ADD COLUMN {};\n\n", quote_ident(table), def);
            if column.index && !column.primary_key && !column.unique {
                out.push_str(&format!(
                    "CREATE INDEX IF NOT EXISTS {} ON {} ({});\n\n",
                    quote_ident(&format!("{}_{}_idx", table, column.name)),
                    quote_ident(table),
                    quote_ident(&column.name)
                ));
            }
            out
        }
        SchemaDiff::DropColumn { table, column } => {
            format!(
                "ALTER TABLE {} DROP COLUMN {};\n\n",
                quote_ident(table),
                quote_ident(column)
            )
        }
        SchemaDiff::AlterColumnType {
            table,
            column,
            new_type,
        } => {
            if backend == BackendKind::Sqlite {
                format!(
                    "-- SQLite does not support ALTER COLUMN TYPE on \"{}\".\"{}\" → recreate the table manually.\n\n",
                    table, column
                )
            } else {
                let ty = sql_type(new_type, backend);
                format!(
                    "ALTER TABLE {} ALTER COLUMN {} TYPE {} USING {}::{};\n\n",
                    quote_ident(table),
                    quote_ident(column),
                    ty,
                    quote_ident(column),
                    ty
                )
            }
        }
        SchemaDiff::AlterColumnNullable {
            table,
            column,
            nullable,
        } => {
            if backend == BackendKind::Sqlite {
                format!(
                    "-- SQLite does not support SET/DROP NOT NULL on \"{}\".\"{}\" → recreate the table manually.\n\n",
                    table, column
                )
            } else {
                let op = if *nullable {
                    "DROP NOT NULL"
                } else {
                    "SET NOT NULL"
                };
                format!(
                    "ALTER TABLE {} ALTER COLUMN {} {};\n\n",
                    quote_ident(table),
                    quote_ident(column),
                    op
                )
            }
        }
        SchemaDiff::AlterColumnUnique {
            table,
            column,
            unique,
        } => {
            if *unique {
                format!(
                    "ALTER TABLE {} ADD CONSTRAINT {} UNIQUE ({});\n\n",
                    quote_ident(table),
                    quote_ident(&format!("{}_{}_unique", table, column)),
                    quote_ident(column)
                )
            } else if backend == BackendKind::Sqlite {
                format!(
                    "DROP INDEX IF EXISTS {};\n\n",
                    quote_ident(&format!("{}_{}_unique", table, column))
                )
            } else {
                format!(
                    "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {};\n\n",
                    quote_ident(table),
                    quote_ident(&format!("{}_{}_unique", table, column))
                )
            }
        }
        SchemaDiff::AlterColumnIndex {
            table,
            column,
            index,
        } => {
            if *index {
                format!(
                    "CREATE INDEX IF NOT EXISTS {} ON {} ({});\n\n",
                    quote_ident(&format!("{}_{}_idx", table, column)),
                    quote_ident(table),
                    quote_ident(column)
                )
            } else {
                format!(
                    "DROP INDEX IF EXISTS {};\n\n",
                    quote_ident(&format!("{}_{}_idx", table, column))
                )
            }
        }
        SchemaDiff::AlterColumnReference {
            table,
            column,
            references,
        } => {
            if backend == BackendKind::Sqlite {
                format!(
                    "-- SQLite does not support ALTER COLUMN REFERENCES on \"{}\".\"{}\" → recreate the table manually.\n\n",
                    table, column
                )
            } else {
                let mut out = format!(
                    "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {};\n",
                    quote_ident(table),
                    quote_ident(&format!("{}_{}_fkey", table, column))
                );
                if let Some(fk) = references {
                    out.push_str(&format!(
                        "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {};\n",
                        quote_ident(table),
                        quote_ident(&format!("{}_{}_fkey", table, column)),
                        quote_ident(column),
                        quote_ident(&fk.table),
                        quote_ident(&fk.column),
                        referential_action_sql(&fk.on_delete)
                    ));
                }
                out.push('\n');
                out
            }
        }
        SchemaDiff::AddConstraint { table, constraint } => {
            if constraint.definition.contains(" WHERE ")
                && let Some((cols, cond)) = parse_unique_definition(&constraint.definition)
            {
                return format!(
                    "CREATE UNIQUE INDEX {} ON {} ({}) WHERE {};\n\n",
                    quote_ident(&constraint.name),
                    quote_ident(table),
                    cols,
                    cond
                );
            }

            if backend == BackendKind::Sqlite {
                format!(
                    "-- SQLite does not support ADD CONSTRAINT \"{}\" on \"{}\" → recreate the table manually.\n\n",
                    constraint.name, table
                )
            } else {
                format!(
                    "ALTER TABLE {} ADD CONSTRAINT {} {};\n\n",
                    quote_ident(table),
                    quote_ident(&constraint.name),
                    constraint.definition
                )
            }
        }
        SchemaDiff::DropConstraint {
            table,
            name,
            partial,
        } => {
            if *partial {
                format!("DROP INDEX IF EXISTS {};\n\n", quote_ident(name))
            } else if backend == BackendKind::Sqlite {
                format!(
                    "-- SQLite does not support DROP CONSTRAINT \"{}\" on \"{}\" → recreate the table manually.\n\n",
                    name, table
                )
            } else {
                format!(
                    "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {};\n\n",
                    quote_ident(table),
                    quote_ident(name)
                )
            }
        }
        SchemaDiff::RenameColumn { table, from, to } => {
            format!(
                "ALTER TABLE {} RENAME COLUMN {} TO {};\n\n",
                quote_ident(table),
                quote_ident(from),
                quote_ident(to)
            )
        }
    }
}

fn generate_create_table(schema: &TableSchema, backend: BackendKind) -> String {
    let mut lines = Vec::new();
    let mut post_stmts: Vec<String> = Vec::new();

    for col in &schema.columns {
        lines.push(format!(
            "    {}",
            column_definition(&schema.table_name, col, backend)
        ));
    }

    // Detect pivot table: 2 UUID NOT NULL non-PK columns → composite PK
    let is_pivot = schema.columns.len() == 2
        && schema
            .columns
            .iter()
            .all(|c| !c.primary_key && !c.nullable && c.col_type == rango_core::ColumnType::Uuid);

    if is_pivot {
        let col1 = &schema.columns[0].name;
        let col2 = &schema.columns[1].name;
        lines.push(format!(
            "    PRIMARY KEY ({}, {})",
            quote_ident(col1),
            quote_ident(col2)
        ));
    }

    // Table-level constraints
    for (i, constraint) in schema.constraints.iter().enumerate() {
        let name = constraint_name(&schema.table_name, constraint, i);
        match constraint {
            Constraint::Check(cc) => {
                lines.push(format!(
                    "    CONSTRAINT {} CHECK ({})",
                    quote_ident(&name),
                    cc.sql
                ));
            }
            Constraint::Unique(uc) if uc.condition.is_none() => {
                let cols = uc
                    .fields
                    .iter()
                    .map(|f| quote_ident(f))
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!(
                    "    CONSTRAINT {} UNIQUE ({})",
                    quote_ident(&name),
                    cols
                ));
            }
            Constraint::Unique(uc) => {
                // Partial unique constraint → separate CREATE UNIQUE INDEX
                let cols = uc
                    .fields
                    .iter()
                    .map(|f| quote_ident(f))
                    .collect::<Vec<_>>()
                    .join(", ");
                let cond = uc.condition.as_deref().unwrap_or("");
                post_stmts.push(format!(
                    "CREATE UNIQUE INDEX {} ON {} ({}) WHERE {};\n",
                    quote_ident(&name),
                    quote_ident(&schema.table_name),
                    cols,
                    cond
                ));
            }
        }
    }

    let mut sql = format!(
        "CREATE TABLE IF NOT EXISTS {} (\n{}\n);\n",
        quote_ident(&schema.table_name),
        lines.join(",\n")
    );

    for stmt in post_stmts {
        sql.push('\n');
        sql.push_str(&stmt);
    }

    for col in &schema.columns {
        if col.index && !col.primary_key && !col.unique {
            sql.push('\n');
            sql.push_str(&format!(
                "CREATE INDEX IF NOT EXISTS {} ON {} ({});\n",
                quote_ident(&format!("{}_{}_idx", schema.table_name, col.name)),
                quote_ident(&schema.table_name),
                quote_ident(&col.name)
            ));
        }
    }

    sql
}

fn column_definition(table: &str, col: &ColumnDef, backend: BackendKind) -> String {
    let mut def = format!(
        "{} {}",
        quote_ident(&col.name),
        sql_type(&col.col_type, backend)
    );
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
            DefaultValue::Literal(v) => def.push_str(&format!(" DEFAULT {}", v)),
            DefaultValue::GeneratedUuid => def.push_str(" DEFAULT gen_random_uuid()"),
        }
    }
    if let Some(fk) = &col.references {
        def.push_str(&format!(
            " CONSTRAINT {} REFERENCES {} ({}) ON DELETE {}",
            quote_ident(&format!("{}_{}_fkey", table, col.name)),
            quote_ident(&fk.table),
            quote_ident(&fk.column),
            referential_action_sql(&fk.on_delete)
        ));
    }
    def
}

fn sql_type(col_type: &ColumnType, backend: BackendKind) -> String {
    match backend {
        BackendKind::Sqlite => match col_type {
            ColumnType::Bool => "INTEGER".into(), // 0/1
            ColumnType::SmallInt => "INTEGER".into(),
            ColumnType::Int => "INTEGER".into(),
            ColumnType::BigInt => "INTEGER".into(),
            ColumnType::Float => "REAL".into(),
            ColumnType::Double => "REAL".into(),
            ColumnType::Decimal { .. } => "REAL".into(),
            ColumnType::Text => "TEXT".into(),
            ColumnType::Bytea => "BLOB".into(),
            ColumnType::Uuid => "TEXT".into(), // stored as UUID string
            ColumnType::Date => "TEXT".into(), // ISO 8601
            ColumnType::Time => "TEXT".into(),
            ColumnType::DateTime => "TEXT".into(), // ISO 8601
            ColumnType::Json | ColumnType::Jsonb => "TEXT".into(), // serialized JSON
            ColumnType::Varchar(_) => "TEXT".into(),
        },
        _ => match col_type {
            ColumnType::Bool => "BOOLEAN".into(),
            ColumnType::SmallInt => "SMALLINT".into(),
            ColumnType::Int => "INTEGER".into(),
            ColumnType::BigInt => "BIGINT".into(),
            ColumnType::Float => "REAL".into(),
            ColumnType::Double => "DOUBLE PRECISION".into(),
            ColumnType::Text => "TEXT".into(),
            ColumnType::Bytea => "BYTEA".into(),
            ColumnType::Uuid => "UUID".into(),
            ColumnType::Date => "DATE".into(),
            ColumnType::Time => "TIME".into(),
            ColumnType::DateTime => "TIMESTAMPTZ".into(),
            ColumnType::Json => "JSON".into(),
            ColumnType::Jsonb => "JSONB".into(),
            ColumnType::Varchar(n) => format!("VARCHAR({})", n),
            ColumnType::Decimal { .. } => "NUMERIC".into(),
        },
    }
}

fn referential_action_sql(action: &ReferentialAction) -> &'static str {
    match action {
        ReferentialAction::Cascade => "CASCADE",
        ReferentialAction::SetNull => "SET NULL",
        ReferentialAction::SetDefault => "SET DEFAULT",
        ReferentialAction::Restrict => "RESTRICT",
        ReferentialAction::NoAction => "NO ACTION",
    }
}

fn parse_unique_definition(definition: &str) -> Option<(String, String)> {
    let rest = definition.strip_prefix("UNIQUE ")?;
    let (cols_part, cond_part) = rest.split_once(" WHERE ")?;
    let cols = cols_part.strip_prefix('(')?.strip_suffix(')')?.to_string();
    Some((cols, cond_part.to_string()))
}

fn sort_create_tables_topologically(schemas: &[TableSchema]) -> Vec<usize> {
    let names: std::collections::HashSet<&str> =
        schemas.iter().map(|s| s.table_name.as_str()).collect();
    let mut placed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut order: Vec<usize> = Vec::new();

    while placed.len() < schemas.len() {
        let mut added = false;
        for (i, schema) in schemas.iter().enumerate() {
            if placed.contains(&schema.table_name) {
                continue;
            }

            let ready = schema
                .columns
                .iter()
                .filter_map(|c| c.references.as_ref())
                .all(|fk| {
                    fk.table == schema.table_name
                        || !names.contains(fk.table.as_str())
                        || placed.contains(&fk.table)
                });

            if ready {
                placed.insert(schema.table_name.clone());
                order.push(i);
                added = true;
                break;
            }
        }

        if !added {
            for (i, schema) in schemas.iter().enumerate() {
                if placed.insert(schema.table_name.clone()) {
                    order.push(i);
                }
            }
            break;
        }
    }

    order
}

#[cfg(test)]
mod tests {
    use super::generate_sql_from_diff;
    use crate::migration::diff::SchemaDiff;
    use crate::migration::scanner::{M2MRelation, generate_pivot_table};
    use rango_core::{ColumnDef, ColumnType, FkReference, ReferentialAction, TableSchema};

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

    fn fk_col(name: &str, table: &str, on_delete: ReferentialAction) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            col_type: ColumnType::Uuid,
            nullable: false,
            primary_key: false,
            unique: false,
            default: None,
            references: Some(FkReference {
                table: table.to_string(),
                column: "id".to_string(),
                on_delete,
                on_update: ReferentialAction::NoAction,
            }),
            index: true,
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
        let sql = generate_sql_from_diff(&[diff], rango_core::BackendKind::Postgres);
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS"), "got: {}", sql);
        assert!(sql.contains("\"users\""), "got: {}", sql);
    }

    #[test]
    fn test_create_table_has_columns() {
        let diff = SchemaDiff::CreateTable(simple_table("orders"));
        let sql = generate_sql_from_diff(&[diff], rango_core::BackendKind::Postgres);
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
        let sql = generate_sql_from_diff(&[diff], rango_core::BackendKind::Postgres);
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
        let sql = generate_sql_from_diff(&[diff], rango_core::BackendKind::Postgres);
        assert!(
            sql.contains("ALTER TABLE \"x\" DROP COLUMN"),
            "got: {}",
            sql
        );
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
        let sql = generate_sql_from_diff(&[diff], rango_core::BackendKind::Postgres);
        assert!(sql.contains("DROP NOT NULL"), "got: {}", sql);
    }

    #[test]
    fn test_alter_nullable_false() {
        let diff = SchemaDiff::AlterColumnNullable {
            table: "t".to_string(),
            column: "c".to_string(),
            nullable: false,
        };
        let sql = generate_sql_from_diff(&[diff], rango_core::BackendKind::Postgres);
        assert!(sql.contains("SET NOT NULL"), "got: {}", sql);
    }

    // ── DropTable ─────────────────────────────────────────────────────────────
    #[test]
    fn test_drop_table_sql() {
        let diff = SchemaDiff::DropTable("old_table".to_string());
        let sql = generate_sql_from_diff(&[diff], rango_core::BackendKind::Postgres);
        assert!(sql.contains("DROP TABLE IF EXISTS"), "got: {}", sql);
        assert!(sql.contains("\"old_table\""), "got: {}", sql);
    }

    // ── Empty diff ────────────────────────────────────────────────────────────
    #[test]
    fn test_empty_diff_produces_empty_string() {
        let sql = generate_sql_from_diff(&[], rango_core::BackendKind::Postgres);
        assert!(
            sql.is_empty(),
            "empty diff should produce empty string; got: {:?}",
            sql
        );
    }

    // ── foreign keys ──────────────────────────────────────────────────────────
    #[test]
    fn test_fk_column_sql_contains_references_cascade() {
        let schema = TableSchema {
            table_name: "posts".to_string(),
            columns: vec![
                uuid_col("id", true),
                fk_col("author", "app_users", ReferentialAction::Cascade),
            ],
            ..Default::default()
        };
        let sql = generate_sql_from_diff(
            &[SchemaDiff::CreateTable(schema)],
            rango_core::BackendKind::Postgres,
        );
        assert!(
            sql.contains("CONSTRAINT \"posts_author_fkey\" REFERENCES \"app_users\" (\"id\") ON DELETE CASCADE"),
            "got: {}",
            sql
        );
    }

    #[test]
    fn test_fk_column_sql_contains_references_set_null() {
        let mut col = fk_col("author", "app_users", ReferentialAction::SetNull);
        col.nullable = true;
        let schema = TableSchema {
            table_name: "posts".to_string(),
            columns: vec![uuid_col("id", true), col],
            ..Default::default()
        };
        let sql = generate_sql_from_diff(
            &[SchemaDiff::CreateTable(schema)],
            rango_core::BackendKind::Postgres,
        );
        assert!(sql.contains("ON DELETE SET NULL"), "got: {}", sql);
    }

    // ── pivot tables ──────────────────────────────────────────────────────────
    #[test]
    fn test_pivot_table_sql_has_composite_pk_references_and_index() {
        let rel = M2MRelation {
            from_table: "posts".to_string(),
            to_table: "tags".to_string(),
        };
        let schema = generate_pivot_table(&rel, "app");
        let sql = generate_sql_from_diff(
            &[SchemaDiff::CreateTable(schema)],
            rango_core::BackendKind::Postgres,
        );

        assert!(
            sql.contains("PRIMARY KEY (\"app_posts_id\", \"app_tags_id\")"),
            "got: {}",
            sql
        );
        assert!(
            sql.contains("REFERENCES \"app_posts\" (\"id\") ON DELETE CASCADE"),
            "got: {}",
            sql
        );
        assert!(
            sql.contains("REFERENCES \"app_tags\" (\"id\") ON DELETE CASCADE"),
            "got: {}",
            sql
        );
        assert!(
            sql.contains("CREATE INDEX IF NOT EXISTS \"app_posts_app_tags_app_tags_id_idx\" ON \"app_posts_app_tags\" (\"app_tags_id\");"),
            "got: {}",
            sql
        );
    }

    // ── topological ordering ──────────────────────────────────────────────────
    #[test]
    fn test_create_tables_are_topologically_ordered() {
        let posts = TableSchema {
            table_name: "posts".to_string(),
            columns: vec![uuid_col("id", true), text_col("title")],
            ..Default::default()
        };
        let comments = TableSchema {
            table_name: "comments".to_string(),
            columns: vec![
                uuid_col("id", true),
                fk_col("post", "posts", ReferentialAction::Cascade),
            ],
            ..Default::default()
        };

        let diffs = vec![
            SchemaDiff::CreateTable(comments),
            SchemaDiff::CreateTable(posts),
        ];
        let sql = generate_sql_from_diff(&diffs, rango_core::BackendKind::Postgres);

        let posts_pos = sql
            .find("CREATE TABLE IF NOT EXISTS \"posts\"")
            .expect("posts table should exist");
        let comments_pos = sql
            .find("CREATE TABLE IF NOT EXISTS \"comments\"")
            .expect("comments table should exist");

        assert!(posts_pos < comments_pos);
    }

    // ── indexes ───────────────────────────────────────────────────────────────
    #[test]
    fn test_indexed_column_emits_create_index() {
        let mut col = text_col("slug");
        col.index = true;
        let schema = TableSchema {
            table_name: "pages".to_string(),
            columns: vec![uuid_col("id", true), col],
            ..Default::default()
        };
        let sql = generate_sql_from_diff(
            &[SchemaDiff::CreateTable(schema)],
            rango_core::BackendKind::Postgres,
        );
        assert!(
            sql.contains("CREATE INDEX IF NOT EXISTS \"pages_slug_idx\" ON \"pages\" (\"slug\");"),
            "got: {}",
            sql
        );
    }

    #[test]
    fn test_fk_column_is_auto_indexed() {
        let schema = TableSchema {
            table_name: "posts".to_string(),
            columns: vec![
                uuid_col("id", true),
                fk_col("author", "app_users", ReferentialAction::Cascade),
            ],
            ..Default::default()
        };
        let sql = generate_sql_from_diff(
            &[SchemaDiff::CreateTable(schema)],
            rango_core::BackendKind::Postgres,
        );
        assert!(
            sql.contains(
                "CREATE INDEX IF NOT EXISTS \"posts_author_idx\" ON \"posts\" (\"author\");"
            ),
            "got: {}",
            sql
        );
    }

    // ── rename + type change SQL ──────────────────────────────────────────────
    #[test]
    fn test_rename_and_type_change_sql() {
        let diffs = vec![
            SchemaDiff::RenameColumn {
                table: "t".to_string(),
                from: "amount".to_string(),
                to: "price".to_string(),
            },
            SchemaDiff::AlterColumnType {
                table: "t".to_string(),
                column: "price".to_string(),
                new_type: ColumnType::Int,
            },
        ];
        let sql = generate_sql_from_diff(&diffs, rango_core::BackendKind::Postgres);

        let rename_pos = sql
            .find("ALTER TABLE \"t\" RENAME COLUMN \"amount\" TO \"price\";")
            .expect("rename should exist");
        let alter_pos = sql
            .find("ALTER TABLE \"t\" ALTER COLUMN \"price\" TYPE INTEGER USING \"price\"::INTEGER;")
            .expect("alter type should exist");

        assert!(rename_pos < alter_pos);
    }

    #[test]
    fn test_pure_type_change_has_using_clause() {
        let diff = SchemaDiff::AlterColumnType {
            table: "t".to_string(),
            column: "value".to_string(),
            new_type: ColumnType::BigInt,
        };
        let sql = generate_sql_from_diff(&[diff], rango_core::BackendKind::Postgres);
        assert!(sql.contains("USING \"value\"::BIGINT"), "got: {}", sql);
    }
}
