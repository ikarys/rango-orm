//! SQLite import: row-by-row inserts, foreign keys deferred to COMMIT.

use super::*;

pub(super) async fn import_sqlite(
    database_url: &str,
    rows: &[serde_json::Map<String, serde_json::Value>],
    table: Option<&str>,
    on_conflict: OnConflict,
    setup_sql: Option<&str>,
) -> Result<usize> {
    let pool = rango_orm_sqlite::connect(database_url)
        .await
        .context("Failed to connect to SQLite")?;

    let mut tx = pool.begin().await.context("Failed to start transaction")?;
    run_setup::<sqlx::Sqlite>(&mut tx, setup_sql).await?;
    // FK checks run at COMMIT, so tables can come in any order. The pragma resets
    // itself at the end of the transaction.
    sqlx::query("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *tx)
        .await
        .context("Failed to defer foreign keys")?;

    let groups = group_by_table(rows, table);
    let mut total = 0;

    for (table, table_rows) in groups {
        total += insert_rows_sqlite(&mut tx, &table, &table_rows, on_conflict).await?;
    }

    tx.commit().await.context("Failed to commit import")?;
    Ok(total)
}

pub(super) async fn insert_rows_sqlite(
    conn: &mut sqlx::SqliteConnection,
    table: &str,
    rows: &[&serde_json::Map<String, serde_json::Value>],
    on_conflict: OnConflict,
) -> Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }

    let cols: Vec<&str> = rows[0]
        .keys()
        .filter(|k| *k != "__table")
        .map(|k| k.as_str())
        .collect();

    let binary = binary_columns_sqlite(&mut *conn, table).await?;
    let mut count = 0;
    for row in rows {
        let col_list = cols
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = cols.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        // INSERT OR REPLACE replaces the whole row. Skipping uses ON CONFLICT DO NOTHING,
        // not INSERT OR IGNORE, which would also swallow NOT NULL / CHECK violations.
        let (verb, suffix) = match on_conflict {
            OnConflict::Replace => ("INSERT OR REPLACE", ""),
            OnConflict::Skip => ("INSERT", " ON CONFLICT DO NOTHING"),
            OnConflict::Fail => ("INSERT", ""),
        };
        let sql = format!(
            "{} INTO {} ({}) VALUES ({}){}",
            verb,
            quote_ident(table),
            col_list,
            placeholders,
            suffix
        );

        let mut q = sqlx::query(sqlx::AssertSqlSafe(&*sql));
        for col in &cols {
            let val = row.get(*col).unwrap_or(&serde_json::Value::Null);
            q = match val {
                serde_json::Value::String(s) if binary.contains(*col) => {
                    q.bind(decode_binary(col, s)?)
                }
                _ => bind_json_value_sqlite(q, val),
            };
        }
        q.execute(&mut *conn)
            .await
            .with_context(|| format!("Failed to insert into {}", table))?;
        count += 1;
    }
    Ok(count)
}

/// Columns declared `BLOB`.
pub(super) async fn binary_columns_sqlite(
    conn: &mut sqlx::SqliteConnection,
    table: &str,
) -> Result<HashSet<String>> {
    let cols: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info(?) WHERE upper(type) = 'BLOB'")
            .bind(table)
            .fetch_all(conn)
            .await
            .context("Failed to read binary columns")?;
    Ok(cols.into_iter().collect())
}

pub(super) fn bind_json_value_sqlite<'q>(
    q: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>,
    val: &serde_json::Value,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    match val {
        serde_json::Value::Null => q.bind(Option::<String>::None),
        serde_json::Value::Bool(b) => q.bind(*b as i64),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                q.bind(i)
            } else if let Some(f) = n.as_f64() {
                q.bind(f)
            } else {
                q.bind(Option::<String>::None)
            }
        }
        serde_json::Value::String(s) => q.bind(s.clone()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => q.bind(val.to_string()),
    }
}
