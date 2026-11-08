//! SQL of one schema diff (ALTER / CREATE INDEX / rebuild), per statement kind.

use super::table::*;
use super::*;

pub(super) fn diff_to_sql(d: &SchemaDiff, backend: BackendKind) -> String {
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
            col_type,
            index,
        } => {
            if *index {
                let key = index_key_of(column, col_type, backend);
                if backend == BackendKind::Mysql {
                    format!(
                        "CREATE INDEX {} ON {} ({});\n\n",
                        object_name(backend, &format!("{}_{}_idx", table, column)),
                        backend.quote_ident(table),
                        key
                    )
                } else {
                    format!(
                        "CREATE INDEX IF NOT EXISTS {} ON {} ({});\n\n",
                        object_name(backend, &format!("{}_{}_idx", table, column)),
                        backend.quote_ident(table),
                        key
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
        SchemaDiff::RebuildTable { schema, copy } => {
            let (create_sql, post_stmts) = create_table_parts(schema, backend);
            let quoted_table = backend.quote_ident(&schema.table_name);
            let quoted_temp = backend.quote_ident(&format!("_rango_new_{}", schema.table_name));
            let old_header = format!("CREATE TABLE IF NOT EXISTS {} (", quoted_table);
            let new_header = format!("CREATE TABLE {} (", quoted_temp);
            let create_sql = create_sql.replacen(&old_header, &new_header, 1);

            let mut out = format!(
                "-- SQLite cannot alter columns or constraints in place: rebuild {}.\n",
                quoted_table
            );
            out.push_str(&create_sql);
            if !copy.is_empty() {
                let cols = copy
                    .iter()
                    .map(|c| backend.quote_ident(c))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(
                    "INSERT INTO {} ({}) SELECT {} FROM {};\n",
                    quoted_temp, cols, cols, quoted_table
                ));
            }
            out.push_str(&format!(
                "DROP TABLE {};\nALTER TABLE {} RENAME TO {};\n",
                quoted_table, quoted_temp, quoted_table
            ));
            for stmt in post_stmts {
                out.push('\n');
                out.push_str(&stmt);
            }
            out.push('\n');
            out
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
