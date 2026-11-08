//! CREATE TABLE, column definitions and SQL types.

use super::*;

pub(super) fn create_table_parts(
    schema: &TableSchema,
    backend: BackendKind,
) -> (String, Vec<String>) {
    let mut lines = Vec::new();
    let mut post_stmts: Vec<String> = Vec::new();

    for col in &schema.columns {
        lines.push(format!(
            "    {}",
            column_definition(&schema.table_name, col, backend)
        ));
    }

    // Detect pivot table: 2 NOT NULL non-PK key columns (UUID, INT or BIGINT) → composite PK
    let is_pivot = schema.columns.len() == 2
        && schema.columns.iter().all(|c| {
            use rango_orm_core::ColumnType::{BigInt, Int, Uuid};
            !c.primary_key && !c.nullable && matches!(c.col_type, Uuid | Int | BigInt)
        });

    if is_pivot {
        let col1 = &schema.columns[0].name;
        let col2 = &schema.columns[1].name;
        lines.push(format!(
            "    PRIMARY KEY ({}, {})",
            backend.quote_ident(col1),
            backend.quote_ident(col2)
        ));
    }

    if backend == BackendKind::Mysql {
        for col in &schema.columns {
            if col.primary_key {
                continue;
            }

            if col.unique {
                lines.push(format!(
                    "    CONSTRAINT {} UNIQUE ({})",
                    object_name(
                        backend,
                        &format!("{}_{}_unique", schema.table_name, col.name)
                    ),
                    backend.quote_ident(&col.name)
                ));
            }

            if col.index && !col.unique {
                lines.push(format!(
                    "    INDEX {} ({})",
                    object_name(backend, &format!("{}_{}_idx", schema.table_name, col.name)),
                    index_key(col, backend)
                ));
            }

            if let Some(fk) = &col.references {
                lines.push(format!(
                    "    CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {}",
                    object_name(backend, &format!("{}_{}_fkey", schema.table_name, col.name)),
                    backend.quote_ident(&col.name),
                    backend.quote_ident(&fk.table),
                    backend.quote_ident(&fk.column),
                    referential_action_sql(&fk.on_delete)
                ));
            }
        }
    }

    // Table-level constraints
    for (i, constraint) in schema.constraints.iter().enumerate() {
        let name = constraint_name(&schema.table_name, constraint, i);
        match constraint {
            Constraint::Check(cc) => {
                lines.push(format!(
                    "    CONSTRAINT {} CHECK ({})",
                    object_name(backend, &name),
                    cc.sql
                ));
            }
            Constraint::Unique(uc) if uc.condition.is_none() => {
                let cols = uc
                    .fields
                    .iter()
                    .map(|f| backend.quote_ident(f))
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!(
                    "    CONSTRAINT {} UNIQUE ({})",
                    object_name(backend, &name),
                    cols
                ));
            }
            Constraint::Unique(uc) => {
                if backend == BackendKind::Mysql {
                    post_stmts.push(format!(
                        "-- MySQL/MariaDB do not support partial unique indexes: constraint \"{}\" on \"{}\" was skipped (enforce it in application code).\n",
                        name, schema.table_name
                    ));
                } else {
                    let cols = uc
                        .fields
                        .iter()
                        .map(|f| backend.quote_ident(f))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let cond = uc.condition.as_deref().unwrap_or("");
                    post_stmts.push(format!(
                        "CREATE UNIQUE INDEX IF NOT EXISTS {} ON {} ({}) WHERE {};\n",
                        object_name(backend, &name),
                        backend.quote_ident(&schema.table_name),
                        cols,
                        cond
                    ));
                }
            }
        }
    }

    let closing = if backend == BackendKind::Mysql {
        ") ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;\n"
    } else {
        ");\n"
    };

    let create_sql = format!(
        "CREATE TABLE IF NOT EXISTS {} (\n{}\n{}",
        backend.quote_ident(&schema.table_name),
        lines.join(",\n"),
        closing
    );

    if backend != BackendKind::Mysql {
        for col in &schema.columns {
            if col.index && !col.primary_key && !col.unique {
                post_stmts.push(format!(
                    "CREATE INDEX IF NOT EXISTS {} ON {} ({});\n",
                    object_name(backend, &format!("{}_{}_idx", schema.table_name, col.name)),
                    backend.quote_ident(&schema.table_name),
                    backend.quote_ident(&col.name)
                ));
            }
        }
    }

    (create_sql, post_stmts)
}

pub(super) fn generate_create_table(schema: &TableSchema, backend: BackendKind) -> String {
    let (create_sql, post_stmts) = create_table_parts(schema, backend);
    let mut sql = create_sql;
    for stmt in post_stmts {
        sql.push('\n');
        sql.push_str(&stmt);
    }
    sql
}

/// Index key part for `col`. MySQL cannot index a TEXT/BLOB column without a prefix length.
pub(super) fn index_key(col: &ColumnDef, backend: BackendKind) -> String {
    index_key_of(&col.name, &col.col_type, backend)
}

pub(super) fn index_key_of(column: &str, col_type: &ColumnType, backend: BackendKind) -> String {
    let name = backend.quote_ident(column);
    match col_type {
        ColumnType::Text | ColumnType::Bytea if backend == BackendKind::Mysql => {
            format!("{name}(191)")
        }
        _ => name,
    }
}

pub(super) fn column_definition(table: &str, col: &ColumnDef, backend: BackendKind) -> String {
    if backend == BackendKind::Mysql {
        let mut def = format!(
            "{} {}",
            backend.quote_ident(&col.name),
            sql_type(&col.col_type, backend)
        );

        if col.primary_key {
            def.push_str(" PRIMARY KEY");
        } else if !col.nullable {
            def.push_str(" NOT NULL");
        }

        if let Some(default) = &col.default {
            match default {
                DefaultValue::CurrentTimestamp => def.push_str(" DEFAULT CURRENT_TIMESTAMP(6)"),
                DefaultValue::Literal(v) => def.push_str(&format!(" DEFAULT {}", v)),
                DefaultValue::GeneratedUuid => {}
            }
        }

        def
    } else {
        let mut def = format!(
            "{} {}",
            backend.quote_ident(&col.name),
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
                // SQLite has no `now()`; its keyword form is accepted as a column default.
                DefaultValue::CurrentTimestamp if backend == BackendKind::Sqlite => {
                    def.push_str(" DEFAULT CURRENT_TIMESTAMP")
                }
                DefaultValue::CurrentTimestamp => def.push_str(" DEFAULT now()"),
                DefaultValue::Literal(v) => def.push_str(&format!(" DEFAULT {}", v)),
                DefaultValue::GeneratedUuid => def.push_str(" DEFAULT gen_random_uuid()"),
            }
        }

        if let Some(fk) = &col.references {
            def.push_str(&format!(
                " CONSTRAINT {} REFERENCES {} ({}) ON DELETE {}",
                object_name(backend, &format!("{}_{}_fkey", table, col.name)),
                backend.quote_ident(&fk.table),
                backend.quote_ident(&fk.column),
                referential_action_sql(&fk.on_delete)
            ));
        }

        def
    }
}

pub(super) fn mysql_modify_column_definition(table: &str, col: &ColumnDef) -> String {
    // The PK index survives MODIFY COLUMN; only restate NOT NULL.
    let mut col = col.clone();
    if col.primary_key {
        col.primary_key = false;
        col.nullable = false;
    }
    column_definition(table, &col, BackendKind::Mysql)
}

pub(super) fn mysql_constraint_definition(definition: &str, backend: BackendKind) -> String {
    if let Some(rest) = definition.strip_prefix("UNIQUE (")
        && let Some(inner) = rest.strip_suffix(')')
    {
        {
            let cols = inner
                .split(", ")
                .map(|raw| {
                    let raw = raw.trim();
                    let unquoted = raw
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .unwrap_or(raw);
                    let unescaped = unquoted.replace("\"\"", "\"");
                    backend.quote_ident(&unescaped)
                })
                .collect::<Vec<_>>()
                .join(", ");
            return format!("UNIQUE ({})", cols);
        }
    }

    definition.to_string()
}

pub(super) fn sql_type(col_type: &ColumnType, backend: BackendKind) -> String {
    match backend {
        BackendKind::Sqlite => match col_type {
            ColumnType::Bool => "INTEGER".into(), // 0/1
            ColumnType::SmallInt => "INTEGER".into(),
            ColumnType::Int => "INTEGER".into(),
            ColumnType::BigInt => "INTEGER".into(),
            // INTEGER PRIMARY KEY is SQLite's rowid alias: the database assigns it.
            ColumnType::AutoInt | ColumnType::AutoBigInt => "INTEGER".into(),
            ColumnType::Float => "REAL".into(),
            ColumnType::Double => "REAL".into(),
            ColumnType::Decimal { .. } => "NUMERIC".into(),
            ColumnType::Text => "TEXT".into(),
            ColumnType::Bytea => "BLOB".into(),
            ColumnType::Uuid => "TEXT".into(), // stored as UUID string
            ColumnType::Date => "TEXT".into(), // ISO 8601
            ColumnType::Time => "TEXT".into(),
            ColumnType::DateTime => "TEXT".into(), // ISO 8601
            ColumnType::Json | ColumnType::Jsonb => "TEXT".into(), // serialized JSON
            ColumnType::Varchar(_) => "TEXT".into(),
        },
        BackendKind::Mysql => match col_type {
            ColumnType::Bool => "BOOLEAN".into(),
            ColumnType::SmallInt => "SMALLINT".into(),
            ColumnType::Int => "INT".into(),
            ColumnType::BigInt => "BIGINT".into(),
            ColumnType::AutoInt => "INT AUTO_INCREMENT".into(),
            ColumnType::AutoBigInt => "BIGINT AUTO_INCREMENT".into(),
            ColumnType::Float => "FLOAT".into(),
            ColumnType::Double => "DOUBLE".into(),
            ColumnType::Decimal { precision, scale } => {
                format!("DECIMAL({}, {})", precision, scale)
            }
            ColumnType::Text => "LONGTEXT".into(),
            ColumnType::Varchar(n) => format!("VARCHAR({})", n),
            ColumnType::Bytea => "LONGBLOB".into(),
            ColumnType::Uuid => "BINARY(16)".into(),
            ColumnType::Date => "DATE".into(),
            ColumnType::Time => "TIME(6)".into(),
            ColumnType::DateTime => "DATETIME(6)".into(),
            ColumnType::Json | ColumnType::Jsonb => "JSON".into(),
        },
        _ => match col_type {
            ColumnType::Bool => "BOOLEAN".into(),
            ColumnType::SmallInt => "SMALLINT".into(),
            ColumnType::Int => "INTEGER".into(),
            ColumnType::BigInt => "BIGINT".into(),
            ColumnType::AutoInt => "INTEGER GENERATED BY DEFAULT AS IDENTITY".into(),
            ColumnType::AutoBigInt => "BIGINT GENERATED BY DEFAULT AS IDENTITY".into(),
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
