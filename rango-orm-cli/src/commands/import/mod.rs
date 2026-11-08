use anyhow::{Context, Result, bail};
use rango_orm_core::{BackendKind, quote_ident, quote_ident_mysql};
use std::collections::{HashMap, HashSet};
use std::fs;

pub async fn run(
    database_url: &str,
    input: &str,
    table: Option<&str>,
    format: Option<&str>,
    replace: bool,
) -> Result<()> {
    let cfg = crate::config::RangoConfig::load().unwrap_or_default();
    let backend = cfg.backend_for(database_url);

    let content = fs::read_to_string(input).with_context(|| format!("Failed to read {}", input))?;

    // Auto-detect format from extension if not specified
    let fmt = match format {
        Some(f) => f.to_string(),
        None => {
            if input.ends_with(".json") {
                "json".to_string()
            } else if input.ends_with(".csv") {
                "csv".to_string()
            } else {
                bail!(
                    "Cannot detect format from filename '{}'. Use --format json|csv.",
                    input
                )
            }
        }
    };

    let rows = match fmt.as_str() {
        "json" => parse::parse_json(&content)?,
        "csv" => parse::parse_csv(&content, table)?,
        other => bail!("Unknown format '{}'. Use 'json' or 'csv'.", other),
    };

    if rows.is_empty() {
        println!("No rows to import.");
        return Ok(());
    }

    if table.is_none() && rows.iter().any(|r| !r.contains_key("__table")) {
        bail!(
            "Rows have no `__table` field (a single-table export, or a CSV, does not record \
             its table): pass the target table with --table <name>"
        );
    }

    let count = insert_rows(
        backend,
        database_url,
        &rows,
        table,
        if replace {
            OnConflict::Replace
        } else {
            OnConflict::Skip
        },
        None,
    )
    .await?;

    println!("✅ Imported {} row(s) from {}", count, input);
    Ok(())
}

/// Insert `rows` in one transaction. `setup_sql` (DDL for `restore`) runs first in that
/// same transaction; MySQL DDL auto-commits, so there it is not rolled back.
/// What to do when a row collides with an existing one.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum OnConflict {
    /// Keep the existing row (`rango import`).
    Skip,
    /// Overwrite it (`rango import --replace`).
    Replace,
    /// Error out and roll back (`rango restore`): dropping rows would be data loss.
    Fail,
}

/// A restore may run on a database `rango migrate` already prepared: ledger rows that exist
/// are skipped, every other conflict still fails.
fn conflict_for(table: &str, on_conflict: OnConflict) -> OnConflict {
    if table == crate::commands::dump::MIGRATIONS_TABLE && on_conflict == OnConflict::Fail {
        OnConflict::Skip
    } else {
        on_conflict
    }
}

pub(crate) async fn insert_rows(
    backend: BackendKind,
    database_url: &str,
    rows: &[serde_json::Map<String, serde_json::Value>],
    table: Option<&str>,
    on_conflict: OnConflict,
    setup_sql: Option<&str>,
) -> Result<usize> {
    match backend {
        BackendKind::Sqlite => {
            sqlite::import_sqlite(database_url, rows, table, on_conflict, setup_sql).await
        }
        BackendKind::Mysql => {
            mysql::import_mysql(database_url, rows, table, on_conflict, setup_sql).await
        }
        _ => postgres::import_postgres(database_url, rows, table, on_conflict, setup_sql).await,
    }
}

async fn run_setup<'c, DB: sqlx::Database>(
    conn: &'c mut DB::Connection,
    setup_sql: Option<&'c str>,
) -> Result<()>
where
    &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
{
    if let Some(sql) = setup_sql {
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(conn)
            .await
            .context("Failed to create the schema")?;
    }
    Ok(())
}

/// Group rows by their `__table` field (or "default" if not present).
fn group_by_table<'a>(
    rows: &'a [serde_json::Map<String, serde_json::Value>],
    default_table: Option<&str>,
) -> std::collections::HashMap<String, Vec<&'a serde_json::Map<String, serde_json::Value>>> {
    let mut groups: std::collections::HashMap<
        String,
        Vec<&serde_json::Map<String, serde_json::Value>>,
    > = std::collections::HashMap::new();

    for row in rows {
        let table = row
            .get("__table")
            .and_then(|v| v.as_str())
            .or(default_table)
            .unwrap_or("default")
            .to_string();
        groups.entry(table).or_default().push(row);
    }
    groups
}

mod binary;
mod mysql;
mod order;
mod parse;
mod postgres;
mod sqlite;

pub(crate) use binary::{decode_binary, encode_binary};
pub(crate) use order::order_by_dependencies;
pub(crate) use postgres::{binary_columns_postgres, columns_of_type_postgres, fk_edges_postgres};

/// Columns of a row to insert: every key except the `__table` marker.
fn insert_columns(row: &serde_json::Map<String, serde_json::Value>) -> Vec<&str> {
    row.keys()
        .filter(|k| *k != "__table")
        .map(String::as_str)
        .collect()
}
