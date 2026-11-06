use crate::migration::diff::SchemaDiff;
use rango_orm_core::{
    BackendKind, ColumnDef, ColumnType, Constraint, DefaultValue, ReferentialAction, TableSchema,
    constraints::constraint_name,
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

    // Postgres/MySQL reject a REFERENCES to a table not created yet (circular FKs): emit
    // those as ALTER TABLE after all CREATEs. SQLite accepts forward references.
    let batch: std::collections::HashSet<&str> =
        creates.iter().map(|s| s.table_name.as_str()).collect();
    let mut created: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut deferred = String::new();

    for i in create_order {
        let mut schema = creates[i].clone();
        created.insert(creates[i].table_name.as_str());
        if backend != BackendKind::Sqlite {
            for col in &mut schema.columns {
                let forward = col.references.as_ref().is_some_and(|fk| {
                    fk.table != schema.table_name
                        && batch.contains(fk.table.as_str())
                        && !created.contains(fk.table.as_str())
                });
                if forward && let Some(fk) = col.references.take() {
                    deferred.push_str(&format!(
                        "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {};\n",
                        backend.quote_ident(&schema.table_name),
                        object_name(backend, &format!("{}_{}_fkey", schema.table_name, col.name)),
                        backend.quote_ident(&col.name),
                        backend.quote_ident(&fk.table),
                        backend.quote_ident(&fk.column),
                        referential_action_sql(&fk.on_delete)
                    ));
                }
            }
        }
        sql.push_str(&generate_create_table(&schema, backend));
        sql.push('\n');
    }
    if !deferred.is_empty() {
        sql.push_str(&deferred);
        sql.push('\n');
    }

    for d in &others {
        sql.push_str(&diff_to_sql(d, backend));
    }

    for d in &drops {
        sql.push_str(&diff_to_sql(d, backend));
    }

    if backend == BackendKind::Mysql && !sql.is_empty() {
        sql.insert_str(
            0,
            "-- MySQL/MariaDB: DDL statements are not transactional; a failing migration can leave earlier statements applied.\n\n",
        );
    }

    sql
}

fn diff_to_sql(d: &SchemaDiff, backend: BackendKind) -> String {
    match d {
        SchemaDiff::CreateTable(schema) => {
            format!("{}\n", generate_create_table(schema, backend))
        }
        SchemaDiff::DropTable(name) => {
            format!("DROP TABLE IF EXISTS {};\n\n", backend.quote_ident(name))
        }
        SchemaDiff::AddColumn { table, column } => {
            if backend == BackendKind::Mysql {
                let def = column_definition(table, column, backend);
                let mut out = format!(
                    "ALTER TABLE {} ADD COLUMN {};\n",
                    backend.quote_ident(table),
                    def
                );

                if column.unique && !column.primary_key {
                    out.push_str(&format!(
                        "ALTER TABLE {} ADD CONSTRAINT {} UNIQUE ({});\n",
                        backend.quote_ident(table),
                        object_name(backend, &format!("{}_{}_unique", table, column.name)),
                        backend.quote_ident(&column.name)
                    ));
                }

                if column.index && !column.primary_key && !column.unique {
                    out.push_str(&format!(
                        "CREATE INDEX {} ON {} ({});\n",
                        object_name(backend, &format!("{}_{}_idx", table, column.name)),
                        backend.quote_ident(table),
                        index_key(column, backend)
                    ));
                }

                if let Some(fk) = &column.references {
                    out.push_str(&format!(
                        "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {};\n",
                        backend.quote_ident(table),
                        object_name(backend, &format!("{}_{}_fkey", table, column.name)),
                        backend.quote_ident(&column.name),
                        backend.quote_ident(&fk.table),
                        backend.quote_ident(&fk.column),
                        referential_action_sql(&fk.on_delete)
                    ));
                }

                out.push('\n');
                out
            } else {
                let def = column_definition(table, column, backend);
                let mut out = format!(
                    "ALTER TABLE {} ADD COLUMN {};\n\n",
                    backend.quote_ident(table),
                    def
                );

                if column.index && !column.primary_key && !column.unique {
                    out.push_str(&format!(
                        "CREATE INDEX IF NOT EXISTS {} ON {} ({});\n\n",
                        object_name(backend, &format!("{}_{}_idx", table, column.name)),
                        backend.quote_ident(table),
                        index_key(column, backend)
                    ));
                }

                out
            }
        }
        SchemaDiff::DropColumn { table, column } => {
            format!(
                "ALTER TABLE {} DROP COLUMN {};\n\n",
                backend.quote_ident(table),
                backend.quote_ident(column)
            )
        }
        SchemaDiff::AlterColumnType { table, column } => {
            if backend == BackendKind::Sqlite {
                format!(
                    "-- SQLite does not support ALTER COLUMN TYPE on \"{}\".\"{}\" → recreate the table manually.\n\n",
                    table, column.name
                )
            } else if backend == BackendKind::Mysql {
                format!(
                    "ALTER TABLE {} MODIFY COLUMN {};\n\n",
                    backend.quote_ident(table),
                    mysql_modify_column_definition(table, column)
                )
            } else {
                let ty = sql_type(&column.col_type, backend);
                format!(
                    "ALTER TABLE {} ALTER COLUMN {} TYPE {} USING {}::{};\n\n",
                    backend.quote_ident(table),
                    backend.quote_ident(&column.name),
                    ty,
                    backend.quote_ident(&column.name),
                    ty
                )
            }
        }
        SchemaDiff::AlterColumnNullable { table, column } => {
            if backend == BackendKind::Sqlite {
                format!(
                    "-- SQLite does not support SET/DROP NOT NULL on \"{}\".\"{}\" → recreate the table manually.\n\n",
                    table, column.name
                )
            } else if backend == BackendKind::Mysql {
                format!(
                    "ALTER TABLE {} MODIFY COLUMN {};\n\n",
                    backend.quote_ident(table),
                    mysql_modify_column_definition(table, column)
                )
            } else {
                let op = if column.nullable {
                    "DROP NOT NULL"
                } else {
                    "SET NOT NULL"
                };
                format!(
                    "ALTER TABLE {} ALTER COLUMN {} {};\n\n",
                    backend.quote_ident(table),
                    backend.quote_ident(&column.name),
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
                    backend.quote_ident(table),
                    object_name(backend, &format!("{}_{}_unique", table, column)),
                    backend.quote_ident(column)
                )
            } else if backend == BackendKind::Sqlite {
                format!(
                    "DROP INDEX IF EXISTS {};\n\n",
                    object_name(backend, &format!("{}_{}_unique", table, column))
                )
            } else if backend == BackendKind::Mysql {
                format!(
                    "ALTER TABLE {} DROP INDEX {};\n\n",
                    backend.quote_ident(table),
                    object_name(backend, &format!("{}_{}_unique", table, column))
                )
            } else {
                format!(
                    "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {};\n\n",
                    backend.quote_ident(table),
                    object_name(backend, &format!("{}_{}_unique", table, column))
                )
            }
        }
        SchemaDiff::AlterColumnIndex {
            table,
            column,
            index,
        } => {
            if *index {
                if backend == BackendKind::Mysql {
                    format!(
                        "CREATE INDEX {} ON {} ({});\n\n",
                        object_name(backend, &format!("{}_{}_idx", table, column)),
                        backend.quote_ident(table),
                        backend.quote_ident(column)
                    )
                } else {
                    format!(
                        "CREATE INDEX IF NOT EXISTS {} ON {} ({});\n\n",
                        object_name(backend, &format!("{}_{}_idx", table, column)),
                        backend.quote_ident(table),
                        backend.quote_ident(column)
                    )
                }
            } else {
                if backend == BackendKind::Mysql {
                    format!(
                        "DROP INDEX {} ON {};\n\n",
                        object_name(backend, &format!("{}_{}_idx", table, column)),
                        backend.quote_ident(table)
                    )
                } else {
                    format!(
                        "DROP INDEX IF EXISTS {};\n\n",
                        object_name(backend, &format!("{}_{}_idx", table, column))
                    )
                }
            }
        }
        SchemaDiff::AlterColumnReference {
            table,
            column,
            references,
            had_reference,
        } => {
            if backend == BackendKind::Sqlite {
                format!(
                    "-- SQLite does not support ALTER COLUMN REFERENCES on \"{}\".\"{}\" → recreate the table manually.\n\n",
                    table, column
                )
            } else if backend == BackendKind::Mysql {
                let mut out = String::new();

                if *had_reference {
                    out.push_str(&format!(
                        "ALTER TABLE {} DROP FOREIGN KEY {};\n",
                        backend.quote_ident(table),
                        object_name(backend, &format!("{}_{}_fkey", table, column))
                    ));
                }

                if let Some(fk) = references {
                    out.push_str(&format!(
                        "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {};\n",
                        backend.quote_ident(table),
                        object_name(backend, &format!("{}_{}_fkey", table, column)),
                        backend.quote_ident(column),
                        backend.quote_ident(&fk.table),
                        backend.quote_ident(&fk.column),
                        referential_action_sql(&fk.on_delete)
                    ));
                }

                out.push('\n');
                out
            } else {
                let mut out = format!(
                    "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {};\n",
                    backend.quote_ident(table),
                    object_name(backend, &format!("{}_{}_fkey", table, column))
                );

                if let Some(fk) = references {
                    out.push_str(&format!(
                        "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {};\n",
                        backend.quote_ident(table),
                        object_name(backend, &format!("{}_{}_fkey", table, column)),
                        backend.quote_ident(column),
                        backend.quote_ident(&fk.table),
                        backend.quote_ident(&fk.column),
                        referential_action_sql(&fk.on_delete)
                    ));
                }

                out.push('\n');
                out
            }
        }
        SchemaDiff::AddConstraint { table, constraint } => {
            if backend == BackendKind::Mysql {
                if constraint.definition.contains(" WHERE ") {
                    return format!(
                        "-- MySQL/MariaDB do not support partial unique indexes: constraint \"{}\" on \"{}\" was skipped (enforce it in application code).\n\n",
                        constraint.name, table
                    );
                }

                let definition = mysql_constraint_definition(&constraint.definition, backend);
                return format!(
                    "ALTER TABLE {} ADD CONSTRAINT {} {};\n\n",
                    backend.quote_ident(table),
                    object_name(backend, &constraint.name),
                    definition
                );
            }

            if constraint.definition.contains(" WHERE ")
                && let Some((cols, cond)) = parse_unique_definition(&constraint.definition)
            {
                return format!(
                    "CREATE UNIQUE INDEX {} ON {} ({}) WHERE {};\n\n",
                    object_name(backend, &constraint.name),
                    backend.quote_ident(table),
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
                    backend.quote_ident(table),
                    object_name(backend, &constraint.name),
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
                if backend == BackendKind::Mysql {
                    format!(
                        "-- partial unique index \"{}\" was never created on MySQL/MariaDB; nothing to drop.\n\n",
                        name
                    )
                } else {
                    format!("DROP INDEX IF EXISTS {};\n\n", object_name(backend, name))
                }
            } else if backend == BackendKind::Sqlite {
                format!(
                    "-- SQLite does not support DROP CONSTRAINT \"{}\" on \"{}\" → recreate the table manually.\n\n",
                    name, table
                )
            } else if backend == BackendKind::Mysql {
                format!(
                    "ALTER TABLE {} DROP CONSTRAINT {};\n\n",
                    backend.quote_ident(table),
                    object_name(backend, name)
                )
            } else {
                format!(
                    "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {};\n\n",
                    backend.quote_ident(table),
                    object_name(backend, name)
                )
            }
        }
        SchemaDiff::RenameColumn { table, from, to } => {
            format!(
                "ALTER TABLE {} RENAME COLUMN {} TO {};\n\n",
                backend.quote_ident(table),
                backend.quote_ident(from),
                backend.quote_ident(to)
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
        && schema.columns.iter().all(|c| {
            !c.primary_key && !c.nullable && c.col_type == rango_orm_core::ColumnType::Uuid
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

    let mut sql = format!(
        "CREATE TABLE IF NOT EXISTS {} (\n{}\n{}",
        backend.quote_ident(&schema.table_name),
        lines.join(",\n"),
        closing
    );

    for stmt in post_stmts {
        sql.push('\n');
        sql.push_str(&stmt);
    }

    if backend != BackendKind::Mysql {
        for col in &schema.columns {
            if col.index && !col.primary_key && !col.unique {
                sql.push('\n');
                sql.push_str(&format!(
                    "CREATE INDEX IF NOT EXISTS {} ON {} ({});\n",
                    object_name(backend, &format!("{}_{}_idx", schema.table_name, col.name)),
                    backend.quote_ident(&schema.table_name),
                    backend.quote_ident(&col.name)
                ));
            }
        }
    }

    sql
}

/// Index key part for `col`. MySQL cannot index a TEXT/BLOB column without a prefix length.
fn index_key(col: &ColumnDef, backend: BackendKind) -> String {
    let name = backend.quote_ident(&col.name);
    match col.col_type {
        ColumnType::Text | ColumnType::Bytea if backend == BackendKind::Mysql => {
            format!("{name}(191)")
        }
        _ => name,
    }
}

fn column_definition(table: &str, col: &ColumnDef, backend: BackendKind) -> String {
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

fn mysql_modify_column_definition(table: &str, col: &ColumnDef) -> String {
    // The PK index survives MODIFY COLUMN; only restate NOT NULL.
    let mut col = col.clone();
    if col.primary_key {
        col.primary_key = false;
        col.nullable = false;
    }
    column_definition(table, &col, BackendKind::Mysql)
}

/// Quoted name of a constraint or index.
///
/// Rango derives these names from table and column names, so they can run past the
/// identifier limit. MySQL rejects names over 64 characters, so longer ones are cut and
/// suffixed with a stable hash of the full name. Postgres truncates every identifier to
/// 63 characters the same way in every statement, so its names are left alone: changing
/// them would break DROPs against existing databases.
fn object_name(backend: BackendKind, name: &str) -> String {
    const MYSQL_MAX_IDENT: usize = 64;
    if backend != BackendKind::Mysql || name.len() <= MYSQL_MAX_IDENT {
        return backend.quote_ident(name);
    }
    // FNV-1a: stable across Rust releases, unlike `DefaultHasher`.
    let hash = name.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    let mut cut = MYSQL_MAX_IDENT - 9;
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    backend.quote_ident(&format!("{}_{:08x}", &name[..cut], hash))
}

fn mysql_constraint_definition(definition: &str, backend: BackendKind) -> String {
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

fn sql_type(col_type: &ColumnType, backend: BackendKind) -> String {
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
    use super::{column_definition, generate_sql_from_diff, object_name};
    use crate::migration::diff::SchemaDiff;
    use crate::migration::scanner::{M2MRelation, generate_pivot_table};
    use crate::migration::snapshot::SnapshotConstraint;
    use rango_orm_core::{
        BackendKind, ColumnDef, ColumnType, FkReference, ReferentialAction, TableSchema,
    };

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

    fn int_col(name: &str) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            col_type: ColumnType::Int,
            nullable: false,
            primary_key: false,
            unique: false,
            default: None,
            references: None,
            index: false,
        }
    }

    fn datetime_col(name: &str) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            col_type: ColumnType::DateTime,
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
        let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS"), "got: {}", sql);
        assert!(sql.contains("\"users\""), "got: {}", sql);
    }

    #[test]
    fn test_create_table_has_columns() {
        let diff = SchemaDiff::CreateTable(simple_table("orders"));
        let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
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
        let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
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
        let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
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
        let mut col = text_col("c");
        col.nullable = true;
        let diff = SchemaDiff::AlterColumnNullable {
            table: "t".to_string(),
            column: col,
        };
        let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
        assert!(sql.contains("DROP NOT NULL"), "got: {}", sql);
    }

    #[test]
    fn test_alter_nullable_false() {
        let diff = SchemaDiff::AlterColumnNullable {
            table: "t".to_string(),
            column: text_col("c"),
        };
        let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
        assert!(sql.contains("SET NOT NULL"), "got: {}", sql);
    }

    // ── DropTable ─────────────────────────────────────────────────────────────
    #[test]
    fn test_drop_table_sql() {
        let diff = SchemaDiff::DropTable("old_table".to_string());
        let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
        assert!(sql.contains("DROP TABLE IF EXISTS"), "got: {}", sql);
        assert!(sql.contains("\"old_table\""), "got: {}", sql);
    }

    // ── Empty diff ────────────────────────────────────────────────────────────
    #[test]
    fn test_empty_diff_produces_empty_string() {
        let sql = generate_sql_from_diff(&[], rango_orm_core::BackendKind::Postgres);
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
            rango_orm_core::BackendKind::Postgres,
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
            rango_orm_core::BackendKind::Postgres,
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
        let schema = generate_pivot_table(&rel);
        let sql = generate_sql_from_diff(
            &[SchemaDiff::CreateTable(schema)],
            rango_orm_core::BackendKind::Postgres,
        );

        assert!(
            sql.contains("PRIMARY KEY (\"posts_id\", \"tags_id\")"),
            "got: {}",
            sql
        );
        assert!(
            sql.contains("REFERENCES \"posts\" (\"id\") ON DELETE CASCADE"),
            "got: {}",
            sql
        );
        assert!(
            sql.contains("REFERENCES \"tags\" (\"id\") ON DELETE CASCADE"),
            "got: {}",
            sql
        );
        assert!(
            sql.contains("CREATE INDEX IF NOT EXISTS \"posts_tags_tags_id_idx\" ON \"posts_tags\" (\"tags_id\");"),
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
        let sql = generate_sql_from_diff(&diffs, rango_orm_core::BackendKind::Postgres);

        let posts_pos = sql
            .find("CREATE TABLE IF NOT EXISTS \"posts\"")
            .expect("posts table should exist");
        let comments_pos = sql
            .find("CREATE TABLE IF NOT EXISTS \"comments\"")
            .expect("comments table should exist");

        assert!(posts_pos < comments_pos);
    }

    // ── circular foreign keys ─────────────────────────────────────────────────
    fn cycle_diffs() -> Vec<SchemaDiff> {
        let table = |name: &str, other: &str| TableSchema {
            table_name: name.to_string(),
            columns: vec![
                uuid_col("id", true),
                fk_col(&format!("{other}_id"), other, ReferentialAction::Cascade),
            ],
            ..Default::default()
        };
        vec![
            SchemaDiff::CreateTable(table("projects", "generations")),
            SchemaDiff::CreateTable(table("generations", "projects")),
        ]
    }

    #[test]
    fn test_circular_fk_deferred_to_alter_table() {
        for backend in [BackendKind::Postgres, BackendKind::Mysql] {
            let sql = generate_sql_from_diff(&cycle_diffs(), backend);
            let q = |n: &str| backend.quote_ident(n);
            let first = sql.find("CREATE TABLE").unwrap();
            let second = first + 1 + sql[first + 1..].find("CREATE TABLE").unwrap();
            let alter = sql.find("ALTER TABLE").expect(&sql);
            assert!(alter > second, "got: {sql}");
            // Only the second table keeps its inline reference (target already created).
            assert_eq!(sql.matches("REFERENCES").count(), 2, "got: {sql}");
            assert_eq!(sql[..alter].matches("REFERENCES").count(), 1, "got: {sql}");
            assert!(
                sql[alter..].contains(&format!(
                    "ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE CASCADE;",
                    q("projects_generations_id_fkey"),
                    q("generations_id"),
                    q("generations"),
                    q("id")
                )),
                "got: {sql}"
            );
        }
    }

    #[test]
    fn test_circular_fk_stays_inline_on_sqlite() {
        let sql = generate_sql_from_diff(&cycle_diffs(), BackendKind::Sqlite);
        assert!(!sql.contains("ALTER TABLE"), "got: {sql}");
        assert_eq!(sql.matches("REFERENCES").count(), 2, "got: {sql}");
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
            rango_orm_core::BackendKind::Postgres,
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
            rango_orm_core::BackendKind::Postgres,
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
                column: int_col("price"),
            },
        ];
        let sql = generate_sql_from_diff(&diffs, rango_orm_core::BackendKind::Postgres);

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
        let mut col = int_col("value");
        col.col_type = ColumnType::BigInt;
        let diff = SchemaDiff::AlterColumnType {
            table: "t".to_string(),
            column: col,
        };
        let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
        assert!(sql.contains("USING \"value\"::BIGINT"), "got: {}", sql);
    }

    // ── MySQL ─────────────────────────────────────────────────────────────────
    #[test]
    fn mysql_create_table_uses_backticks_engine_and_inline_index() {
        let mut col = text_col("slug");
        col.index = true;
        let schema = TableSchema {
            table_name: "pages".to_string(),
            columns: vec![uuid_col("id", true), datetime_col("created_at"), col],
            ..Default::default()
        };

        let sql = generate_sql_from_diff(&[SchemaDiff::CreateTable(schema)], BackendKind::Mysql);

        assert!(
            sql.contains("CREATE TABLE IF NOT EXISTS `pages`"),
            "got: {}",
            sql
        );
        assert!(sql.contains("BINARY(16)"), "got: {}", sql);
        assert!(sql.contains("DATETIME(6)"), "got: {}", sql);
        assert!(
            sql.contains("ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;"),
            "got: {}",
            sql
        );
        assert!(
            sql.contains("INDEX `pages_slug_idx` (`slug`(191))"),
            "got: {}",
            sql
        );
        assert!(!sql.contains("CREATE INDEX IF NOT EXISTS"), "got: {}", sql);
    }

    #[test]
    fn mysql_create_table_uses_table_level_foreign_key() {
        let schema = TableSchema {
            table_name: "posts".to_string(),
            columns: vec![
                uuid_col("id", true),
                fk_col("author", "app_users", ReferentialAction::Cascade),
            ],
            ..Default::default()
        };

        let sql = generate_sql_from_diff(&[SchemaDiff::CreateTable(schema)], BackendKind::Mysql);

        assert!(
            sql.contains(
                "CONSTRAINT `posts_author_fkey` FOREIGN KEY (`author`) REFERENCES `app_users` (`id`) ON DELETE CASCADE"
            ),
            "got: {}",
            sql
        );
    }

    #[test]
    fn mysql_alter_column_type_uses_modify_column() {
        let diff = SchemaDiff::AlterColumnType {
            table: "t".to_string(),
            column: int_col("c"),
        };

        let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

        assert!(
            sql.contains("ALTER TABLE `t` MODIFY COLUMN `c` INT NOT NULL;"),
            "got: {}",
            sql
        );
    }

    #[test]
    fn mysql_alter_column_nullable_uses_modify_column() {
        let mut col = text_col("c");
        col.nullable = true;
        let diff = SchemaDiff::AlterColumnNullable {
            table: "t".to_string(),
            column: col,
        };

        let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

        assert!(
            sql.contains("ALTER TABLE `t` MODIFY COLUMN `c` LONGTEXT;"),
            "got: {}",
            sql
        );
    }

    #[test]
    fn mysql_alter_column_reference_drops_foreign_key_only_when_had_reference() {
        let fk = FkReference {
            table: "users".to_string(),
            column: "id".to_string(),
            on_delete: ReferentialAction::Cascade,
            on_update: ReferentialAction::NoAction,
        };

        let with_old_fk = SchemaDiff::AlterColumnReference {
            table: "t".to_string(),
            column: "c".to_string(),
            references: Some(fk.clone()),
            had_reference: true,
        };
        let sql = generate_sql_from_diff(&[with_old_fk], BackendKind::Mysql);
        assert!(
            sql.contains("ALTER TABLE `t` DROP FOREIGN KEY `t_c_fkey`;"),
            "got: {}",
            sql
        );
        assert!(
            sql.contains(
                "ALTER TABLE `t` ADD CONSTRAINT `t_c_fkey` FOREIGN KEY (`c`) REFERENCES `users` (`id`) ON DELETE CASCADE;"
            ),
            "got: {}",
            sql
        );

        let without_old_fk = SchemaDiff::AlterColumnReference {
            table: "t".to_string(),
            column: "c".to_string(),
            references: Some(fk),
            had_reference: false,
        };
        let sql = generate_sql_from_diff(&[without_old_fk], BackendKind::Mysql);
        assert!(!sql.contains("DROP FOREIGN KEY"), "got: {}", sql);
        assert!(
            sql.contains(
                "ALTER TABLE `t` ADD CONSTRAINT `t_c_fkey` FOREIGN KEY (`c`) REFERENCES `users` (`id`) ON DELETE CASCADE;"
            ),
            "got: {}",
            sql
        );
    }

    #[test]
    fn mysql_alter_column_index_uses_drop_index_on() {
        let diff = SchemaDiff::AlterColumnIndex {
            table: "t".to_string(),
            column: "c".to_string(),
            index: false,
        };

        let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

        assert!(sql.contains("DROP INDEX `t_c_idx` ON `t`;"), "got: {}", sql);
        assert!(!sql.contains("DROP INDEX IF EXISTS"), "got: {}", sql);
    }

    #[test]
    fn mysql_add_constraint_unique_requotes_columns() {
        let diff = SchemaDiff::AddConstraint {
            table: "t".to_string(),
            constraint: SnapshotConstraint {
                name: "t_room_date_unique".to_string(),
                definition: "UNIQUE (\"room\", \"date\")".to_string(),
            },
        };

        let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

        assert!(
            sql.contains(
                "ALTER TABLE `t` ADD CONSTRAINT `t_room_date_unique` UNIQUE (`room`, `date`);"
            ),
            "got: {}",
            sql
        );
    }

    #[test]
    fn mysql_partial_unique_constraint_is_skipped_with_comment() {
        let diff = SchemaDiff::AddConstraint {
            table: "t".to_string(),
            constraint: SnapshotConstraint {
                name: "t_active_unique".to_string(),
                definition: "UNIQUE (\"email\") WHERE active".to_string(),
            },
        };

        let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

        assert!(
            sql.contains(
                "-- MySQL/MariaDB do not support partial unique indexes: constraint \"t_active_unique\" on \"t\" was skipped (enforce it in application code)."
            ),
            "got: {}",
            sql
        );
    }

    #[test]
    fn mysql_non_empty_sql_has_header_comment() {
        let diff = SchemaDiff::DropTable("t".to_string());
        let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

        assert!(
            sql.starts_with(
                "-- MySQL/MariaDB: DDL statements are not transactional; a failing migration can leave earlier statements applied.\n\n"
            ),
            "got: {}",
            sql
        );
    }

    #[test]
    fn mysql_identifier_with_backtick_is_doubled() {
        let diff = SchemaDiff::CreateTable(simple_table("we`ird"));
        let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

        assert!(sql.contains("`we``ird`"), "got: {}", sql);
    }

    #[test]
    fn mysql_long_object_names_are_shortened_with_a_stable_hash() {
        let long = format!("{}_fkey", "a".repeat(70));
        let quoted = object_name(BackendKind::Mysql, &long);
        let name = quoted.trim_matches('`');
        assert_eq!(name.len(), 64);
        assert!(name.starts_with(&"a".repeat(55)));
        assert_eq!(quoted, object_name(BackendKind::Mysql, &long));
        assert_ne!(
            quoted,
            object_name(BackendKind::Mysql, &format!("{}_idx", "a".repeat(70)))
        );
        assert_eq!(
            object_name(BackendKind::Postgres, &long),
            format!("\"{long}\"")
        );
        assert_eq!(object_name(BackendKind::Mysql, "t_c_idx"), "`t_c_idx`");
    }

    #[test]
    fn auto_pk_ddl_per_backend() {
        let col = ColumnDef {
            name: "id".into(),
            col_type: ColumnType::AutoBigInt,
            nullable: false,
            primary_key: true,
            unique: true,
            default: None,
            references: None,
            index: false,
        };
        assert_eq!(
            column_definition("t", &col, BackendKind::Postgres),
            "\"id\" BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY"
        );
        assert_eq!(
            column_definition("t", &col, BackendKind::Mysql),
            "`id` BIGINT AUTO_INCREMENT PRIMARY KEY"
        );
        assert_eq!(
            column_definition("t", &col, BackendKind::Sqlite),
            "\"id\" INTEGER PRIMARY KEY"
        );
    }
}
