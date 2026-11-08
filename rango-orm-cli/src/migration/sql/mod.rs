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
                    // Postgres only: lets `restore` load a NOT NULL cycle with
                    // `SET CONSTRAINTS ALL DEFERRED`. Still checked per statement otherwise.
                    let deferrable = if backend == BackendKind::Postgres {
                        " DEFERRABLE INITIALLY IMMEDIATE"
                    } else {
                        ""
                    };
                    deferred.push_str(&format!(
                        "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {}{};\n",
                        backend.quote_ident(&schema.table_name),
                        object_name(backend, &format!("{}_{}_fkey", schema.table_name, col.name)),
                        backend.quote_ident(&col.name),
                        backend.quote_ident(&fk.table),
                        backend.quote_ident(&fk.column),
                        referential_action_sql(&fk.on_delete),
                        deferrable
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

mod diff_sql;
mod table;
#[cfg(test)]
mod tests;

use diff_sql::diff_to_sql;
use table::generate_create_table;

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
