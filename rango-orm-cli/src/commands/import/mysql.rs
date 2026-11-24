//! MySQL / MariaDB import: foreign key checks off during inserts, checked before commit; UUIDs as `BINARY(16)`.

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
    let tables: Vec<String> = groups.keys().cloned().collect();
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

    check_foreign_keys_mysql(&mut tx, &tables).await?;
    sqlx::query("SET FOREIGN_KEY_CHECKS = 1")
        .execute(&mut *tx)
        .await
        .context("Failed to re-enable foreign key checks")?;
    tx.commit().await.context("Failed to commit import")?;
    Ok(total)
}

async fn check_foreign_keys_mysql(
    conn: &mut sqlx::MySqlConnection,
    tables: &[String],
) -> Result<()> {
    use sqlx::Row;

    let fks = sqlx::query(sqlx::AssertSqlSafe(
        "SELECT CAST(CONSTRAINT_NAME AS CHAR), CAST(TABLE_NAME AS CHAR), \
         CAST(COLUMN_NAME AS CHAR), CAST(REFERENCED_TABLE_NAME AS CHAR), \
         CAST(REFERENCED_COLUMN_NAME AS CHAR) \
         FROM information_schema.KEY_COLUMN_USAGE \
         WHERE TABLE_SCHEMA = DATABASE() AND REFERENCED_TABLE_NAME IS NOT NULL \
         ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION",
    ))
    .fetch_all(&mut *conn)
    .await
    .context("Failed to read foreign keys")?;

    let table_set: HashSet<&str> = tables.iter().map(String::as_str).collect();
    struct ForeignKey {
        child: String,
        constraint: String,
        parent: String,
        pairs: Vec<(String, String)>,
    }
    let mut groups: Vec<ForeignKey> = Vec::new();
    for row in fks {
        let constraint: String = row.try_get(0)?;
        let table: String = row.try_get(1)?;
        let column: String = row.try_get(2)?;
        let referenced_table: String = row.try_get(3)?;
        let referenced_column: String = row.try_get(4)?;
        if !table_set.contains(table.as_str()) {
            continue;
        }
        if let Some(group) = groups
            .iter_mut()
            .find(|g| g.child == table && g.constraint == constraint)
        {
            group.pairs.push((column, referenced_column));
        } else {
            groups.push(ForeignKey {
                child: table,
                constraint,
                parent: referenced_table,
                pairs: vec![(column, referenced_column)],
            });
        }
    }

    for ForeignKey {
        child,
        constraint,
        parent,
        pairs,
    } in groups
    {
        let not_null = pairs
            .iter()
            .map(|(c, _)| format!("c.{} IS NOT NULL", quote_ident_mysql(c)))
            .collect::<Vec<_>>()
            .join(" AND ");
        let matches = pairs
            .iter()
            // Qualified: a parent column with the child's name (`id`) would shadow it.
            .map(|(c, p)| format!("p.{} = c.{}", quote_ident_mysql(p), quote_ident_mysql(c)))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!(
            "SELECT COUNT(*) FROM {child} c WHERE {not_null} AND NOT EXISTS \
             (SELECT 1 FROM {parent} p WHERE {matches})",
            child = quote_ident_mysql(&child),
            parent = quote_ident_mysql(&parent),
        );
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(&*sql))
            .fetch_one(&mut *conn)
            .await
            .with_context(|| format!("Failed to check foreign key {constraint}"))?;
        if n > 0 {
            bail!(
                "foreign key `{constraint}`: {n} row(s) of `{child}` point to a missing row of `{parent}`; nothing was imported"
            );
        }
    }
    Ok(())
}

async fn count_rows_mysql(conn: &mut sqlx::MySqlConnection, table: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {}", quote_ident_mysql(table));
    sqlx::query_scalar(sqlx::AssertSqlSafe(&*sql))
        .fetch_one(&mut *conn)
        .await
        .context("Failed to count rows")
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
    let col_list = cols
        .iter()
        .map(|c| quote_ident_mysql(c))
        .collect::<Vec<_>>()
        .join(", ");
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

    let count_before = if on_conflict == OnConflict::Skip {
        Some(count_rows_mysql(&mut *conn, table).await?)
    } else {
        None
    };
    for chunk in rows.chunks(rows_per_statement(65535, cols.len())) {
        let sql = format!(
            "INSERT INTO {} ({}) VALUES {} {}",
            quote_ident_mysql(table),
            col_list,
            values_placeholders(chunk.len(), cols.len()),
            conflict
        );
        let mut q = sqlx::query(sqlx::AssertSqlSafe(&*sql));
        for row in chunk {
            for col in &cols {
                q = bind_json_value_mysql(
                    q,
                    row.get(*col).unwrap_or(&serde_json::Value::Null),
                    col,
                    uuid_cols.contains(*col),
                    binary.contains(*col),
                )?;
            }
        }
        q.execute(&mut *conn)
            .await
            .with_context(|| format!("Failed to insert into {}", table))?;
    }
    if let Some(before) = count_before {
        let after = count_rows_mysql(&mut *conn, table).await?;
        Ok(after.saturating_sub(before) as usize)
    } else {
        Ok(rows.len())
    }
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
