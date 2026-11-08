//! MySQL / MariaDB import: foreign key checks off, UUIDs as `BINARY(16)`.

use super::binary::uuid_text_to_bytes;
use super::*;

pub(super) async fn import_mysql(
    database_url: &str,
    rows: &[serde_json::Map<String, serde_json::Value>],
    table: Option<&str>,
    on_conflict: OnConflict,
    setup_sql: Option<&str>,
) -> Result<usize> {
    let pool = rango_orm_mysql::connect(database_url)
        .await
        .context("Failed to connect to MySQL")?;

    // FK checks off (session-scoped, like mysqldump): rows can arrive in any table
    // order. The DDL runs first, outside the transaction: it would implicitly commit it.
    let mut conn = pool
        .acquire()
        .await
        .context("Failed to acquire a connection")?;
    sqlx::query("SET FOREIGN_KEY_CHECKS = 0")
        .execute(&mut *conn)
        .await
        .context("Failed to disable foreign key checks")?;
    run_setup::<sqlx::MySql>(&mut conn, setup_sql).await?;
    let mut tx = sqlx::Connection::begin(&mut *conn)
        .await
        .context("Failed to start transaction")?;

    let groups = group_by_table(rows, table);
    let mut total = 0;
    for (table, table_rows) in groups {
        total += insert_rows_mysql(
            &mut tx,
            &table,
            &table_rows,
            conflict_for(&table, on_conflict),
        )
        .await?;
    }

    sqlx::query("SET FOREIGN_KEY_CHECKS = 1")
        .execute(&mut *tx)
        .await
        .context("Failed to re-enable foreign key checks")?;
    tx.commit().await.context("Failed to commit import")?;
    Ok(total)
}

pub(super) async fn insert_rows_mysql(
    conn: &mut sqlx::MySqlConnection,
    table: &str,
    rows: &[&serde_json::Map<String, serde_json::Value>],
    on_conflict: OnConflict,
) -> Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }

    let cols = insert_columns(rows[0]);

    if cols.is_empty() {
        return Ok(0);
    }

    let uuid_cols = fetch_uuid_columns_mysql(&mut *conn, table).await?;
    let binary = binary_columns_mysql(&mut *conn, table).await?;
    let mut count = 0;

    for row in rows {
        let col_list = cols
            .iter()
            .map(|c| quote_ident_mysql(c))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = cols.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let conflict = match on_conflict {
            OnConflict::Replace => {
                let updates = cols
                    .iter()
                    .map(|c| {
                        format!(
                            "{} = VALUES({})",
                            quote_ident_mysql(c),
                            quote_ident_mysql(c)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("ON DUPLICATE KEY UPDATE {}", updates)
            }
            OnConflict::Skip => format!(
                "ON DUPLICATE KEY UPDATE {} = {}",
                quote_ident_mysql(cols[0]),
                quote_ident_mysql(cols[0])
            ),
            OnConflict::Fail => String::new(),
        };
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({}) {}",
            quote_ident_mysql(table),
            col_list,
            placeholders,
            conflict
        );

        let mut q = sqlx::query(sqlx::AssertSqlSafe(&*sql));
        for col in &cols {
            q = bind_json_value_mysql(
                q,
                row.get(*col).unwrap_or(&serde_json::Value::Null),
                col,
                uuid_cols.contains(*col),
                binary.contains(*col),
            )?;
        }
        q.execute(&mut *conn)
            .await
            .with_context(|| format!("Failed to insert into {}", table))?;
        count += 1;
    }
    Ok(count)
}

pub(super) async fn fetch_uuid_columns_mysql(
    conn: &mut sqlx::MySqlConnection,
    table: &str,
) -> Result<HashSet<String>> {
    use sqlx::Row;

    let rows = sqlx::query(sqlx::AssertSqlSafe(
        "SELECT CAST(COLUMN_NAME AS CHAR) FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = DATABASE() \
           AND TABLE_NAME = ? \
           AND DATA_TYPE = 'binary' \
           AND CHARACTER_OCTET_LENGTH = 16",
    ))
    .bind(table)
    .fetch_all(conn)
    .await
    .context("Failed to fetch UUID columns")?;

    Ok(rows
        .into_iter()
        .map(|r| r.try_get::<String, _>(0).unwrap_or_default())
        .collect())
}

pub(super) fn bind_json_value_mysql<'q>(
    q: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    val: &serde_json::Value,
    column: &str,
    is_uuid_col: bool,
    is_binary_col: bool,
) -> Result<sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>> {
    match val {
        serde_json::Value::Null => Ok(q.bind(Option::<String>::None)),
        serde_json::Value::Bool(b) => Ok(q.bind(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(q.bind(i))
            } else if let Some(f) = n.as_f64() {
                Ok(q.bind(f))
            } else {
                Ok(q.bind(Option::<String>::None))
            }
        }
        serde_json::Value::String(s) => {
            if is_uuid_col {
                match uuid_text_to_bytes(s) {
                    Some(bytes) => Ok(q.bind(bytes)),
                    None => bail!(
                        "Column '{}' contains a non-UUID string '{}'; expected a UUID for a BINARY(16) column",
                        column,
                        s
                    ),
                }
            } else if is_binary_col {
                Ok(q.bind(decode_binary(column, s)?))
            } else {
                Ok(q.bind(s.clone()))
            }
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Ok(q.bind(val.to_string())),
    }
}

/// BLOB / VARBINARY columns (the 16-byte `BINARY` UUID columns are handled apart).
pub(super) async fn binary_columns_mysql(
    conn: &mut sqlx::MySqlConnection,
    table: &str,
) -> Result<HashSet<String>> {
    let cols: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(
        "SELECT CAST(COLUMN_NAME AS CHAR) FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? \
           AND DATA_TYPE IN ('tinyblob', 'blob', 'mediumblob', 'longblob', 'varbinary', 'binary') \
           AND NOT (DATA_TYPE = 'binary' AND CHARACTER_OCTET_LENGTH = 16)",
    ))
    .bind(table)
    .fetch_all(conn)
    .await
    .context("Failed to read binary columns")?;
    Ok(cols.into_iter().collect())
}
