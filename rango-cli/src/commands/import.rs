use anyhow::{Context, Result, bail};
use rango_core::{BackendKind, quote_ident, quote_ident_mysql};
use std::collections::HashSet;
use std::fs;

pub async fn run(
    database_url: &str,
    input: &str,
    table: Option<&str>,
    format: Option<&str>,
    replace: bool,
) -> Result<()> {
    let cfg = crate::config::RangoConfig::load().unwrap_or_default();
    let backend = cfg.database.backend_kind();

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
        "json" => parse_json(&content)?,
        "csv" => parse_csv(&content, table)?,
        other => bail!("Unknown format '{}'. Use 'json' or 'csv'.", other),
    };

    if rows.is_empty() {
        println!("No rows to import.");
        return Ok(());
    }

    let count = match backend {
        BackendKind::Sqlite => import_sqlite(database_url, &rows, table, replace).await?,
        BackendKind::Mysql => import_mysql(database_url, &rows, table, replace).await?,
        _ => import_postgres(database_url, &rows, table, replace).await?,
    };

    println!("✅ Imported {} row(s) from {}", count, input);
    Ok(())
}

// ─── Parsers ──────────────────────────────────────────────────────────────────

fn parse_json(content: &str) -> Result<Vec<serde_json::Map<String, serde_json::Value>>> {
    let value: serde_json::Value = serde_json::from_str(content).context("Failed to parse JSON")?;

    match value {
        serde_json::Value::Array(arr) => arr
            .into_iter()
            .map(|v| match v {
                serde_json::Value::Object(map) => Ok(map),
                _ => bail!("Expected array of objects"),
            })
            .collect(),
        serde_json::Value::Object(map) => Ok(vec![map]),
        _ => bail!("Expected JSON array or object"),
    }
}

fn parse_csv(
    content: &str,
    table: Option<&str>,
) -> Result<Vec<serde_json::Map<String, serde_json::Value>>> {
    let mut lines = content.lines();
    let headers: Vec<&str> = match lines.next() {
        Some(h) => h.split(',').collect(),
        None => return Ok(vec![]),
    };

    lines
        .map(|line| {
            let values: Vec<&str> = line.splitn(headers.len(), ',').collect();
            let mut map = serde_json::Map::new();
            if let Some(t) = table {
                map.insert(
                    "__table".to_string(),
                    serde_json::Value::String(t.to_string()),
                );
            }
            for (i, header) in headers.iter().enumerate() {
                let val = values.get(i).copied().unwrap_or("");
                let json_val = if val.is_empty() {
                    serde_json::Value::Null
                } else if let Ok(n) = val.parse::<i64>() {
                    serde_json::Value::Number(n.into())
                } else if let Ok(f) = val.parse::<f64>() {
                    serde_json::Number::from_f64(f)
                        .map(serde_json::Value::Number)
                        .unwrap_or_else(|| serde_json::Value::String(val.to_string()))
                } else if val == "true" || val == "false" {
                    serde_json::Value::Bool(val == "true")
                } else {
                    // Strip surrounding quotes (CSV escaping)
                    let s = if val.starts_with('"') && val.ends_with('"') {
                        val[1..val.len() - 1].replace("\"\"", "\"")
                    } else {
                        val.to_string()
                    };
                    serde_json::Value::String(s)
                };
                map.insert(header.trim().to_string(), json_val);
            }
            Ok(map)
        })
        .collect()
}

// ─── Postgres import ──────────────────────────────────────────────────────────

async fn import_postgres(
    database_url: &str,
    rows: &[serde_json::Map<String, serde_json::Value>],
    table: Option<&str>,
    replace: bool,
) -> Result<usize> {
    let pool = sqlx::PgPool::connect(database_url)
        .await
        .context("Failed to connect to Postgres")?;

    // Postgres cannot disable FK checks without superuser, so parents are inserted
    // first. One transaction, so a failed import leaves nothing behind.
    let mut tx = pool.begin().await.context("Failed to start transaction")?;
    let edges = fk_edges_postgres(&mut tx).await?;

    // Group rows by table
    let groups = group_by_table(rows, table);
    let order = order_by_dependencies(groups.keys().cloned().collect(), &edges);
    let mut total = 0;

    for table in &order {
        total += insert_rows_postgres(&mut tx, table, &groups[table], replace).await?;
    }
    resync_identities_postgres(&mut tx, &order).await?;

    tx.commit().await.context("Failed to commit import")?;
    Ok(total)
}

/// Move each identity sequence of `tables` past the largest imported id.
async fn resync_identities_postgres(
    conn: &mut sqlx::PgConnection,
    tables: &[String],
) -> Result<()> {
    let identities: Vec<(String, String)> = sqlx::query_as(
        "SELECT table_name::text, column_name::text FROM information_schema.columns \
         WHERE table_schema = current_schema() AND is_identity = 'YES'",
    )
    .fetch_all(&mut *conn)
    .await
    .context("Failed to read identity columns")?;
    for (table, column) in identities.iter().filter(|(t, _)| tables.contains(t)) {
        let sql = format!(
            "SELECT setval(pg_get_serial_sequence($1, $2), COALESCE((SELECT MAX({}) FROM {}), 0) + 1, false)",
            quote_ident(column),
            quote_ident(table)
        );
        sqlx::query(&sql)
            .bind(quote_ident(table))
            .bind(column)
            .execute(&mut *conn)
            .await
            .with_context(|| format!("Failed to reset the identity sequence of {}", table))?;
    }
    Ok(())
}

async fn insert_rows_postgres(
    conn: &mut sqlx::PgConnection,
    table: &str,
    rows: &[&serde_json::Map<String, serde_json::Value>],
    replace: bool,
) -> Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }

    let cols: Vec<&str> = rows[0]
        .keys()
        .filter(|k| *k != "__table")
        .map(|k| k.as_str())
        .collect();

    let mut count = 0;
    for row in rows {
        let col_list = cols
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = (1..=cols.len())
            .map(|i| format!("${}", i))
            .collect::<Vec<_>>()
            .join(", ");
        let conflict = if replace {
            let updates = cols
                .iter()
                .map(|c| format!("{} = EXCLUDED.{}", quote_ident(c), quote_ident(c)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("ON CONFLICT DO UPDATE SET {}", updates)
        } else {
            "ON CONFLICT DO NOTHING".to_string()
        };
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({}) {}",
            quote_ident(table),
            col_list,
            placeholders,
            conflict
        );

        let mut q = sqlx::query(&sql);
        for col in &cols {
            q = bind_json_value_pg(q, row.get(*col).unwrap_or(&serde_json::Value::Null));
        }
        q.execute(&mut *conn)
            .await
            .with_context(|| format!("Failed to insert into {}", table))?;
        count += 1;
    }
    Ok(count)
}

fn bind_json_value_pg<'q>(
    q: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    val: &serde_json::Value,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match val {
        serde_json::Value::Null => q.bind(Option::<i64>::None),
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

// ─── SQLite import ────────────────────────────────────────────────────────────

async fn import_sqlite(
    database_url: &str,
    rows: &[serde_json::Map<String, serde_json::Value>],
    table: Option<&str>,
    replace: bool,
) -> Result<usize> {
    let pool = sqlx::SqlitePool::connect(database_url)
        .await
        .context("Failed to connect to SQLite")?;

    let mut tx = pool.begin().await.context("Failed to start transaction")?;
    // FK checks run at COMMIT, so tables can come in any order. The pragma resets
    // itself at the end of the transaction.
    sqlx::query("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *tx)
        .await
        .context("Failed to defer foreign keys")?;

    let groups = group_by_table(rows, table);
    let mut total = 0;

    for (table, table_rows) in groups {
        total += insert_rows_sqlite(&mut tx, &table, &table_rows, replace).await?;
    }

    tx.commit().await.context("Failed to commit import")?;
    Ok(total)
}

async fn insert_rows_sqlite(
    conn: &mut sqlx::SqliteConnection,
    table: &str,
    rows: &[&serde_json::Map<String, serde_json::Value>],
    replace: bool,
) -> Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }

    let cols: Vec<&str> = rows[0]
        .keys()
        .filter(|k| *k != "__table")
        .map(|k| k.as_str())
        .collect();

    let mut count = 0;
    for row in rows {
        let col_list = cols
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = cols.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        // SQLite: INSERT OR REPLACE replaces the whole row, INSERT OR IGNORE skips on conflict
        let verb = if replace {
            "INSERT OR REPLACE"
        } else {
            "INSERT OR IGNORE"
        };
        let sql = format!(
            "{} INTO {} ({}) VALUES ({})",
            verb,
            quote_ident(table),
            col_list,
            placeholders
        );

        let mut q = sqlx::query(&sql);
        for col in &cols {
            q = bind_json_value_sqlite(q, row.get(*col).unwrap_or(&serde_json::Value::Null));
        }
        q.execute(&mut *conn)
            .await
            .with_context(|| format!("Failed to insert into {}", table))?;
        count += 1;
    }
    Ok(count)
}

fn bind_json_value_sqlite<'q>(
    q: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    val: &serde_json::Value,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
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

// ─── MySQL import ─────────────────────────────────────────────────────────────

async fn import_mysql(
    database_url: &str,
    rows: &[serde_json::Map<String, serde_json::Value>],
    table: Option<&str>,
    replace: bool,
) -> Result<usize> {
    let pool = rango_mysql::connect(database_url)
        .await
        .context("Failed to connect to MySQL")?;

    // One transaction with FK checks off (session-scoped, like mysqldump): rows can
    // arrive in any table order, and a failed import leaves nothing behind.
    let mut tx = pool.begin().await.context("Failed to start transaction")?;
    sqlx::query("SET FOREIGN_KEY_CHECKS = 0")
        .execute(&mut *tx)
        .await
        .context("Failed to disable foreign key checks")?;

    let groups = group_by_table(rows, table);
    let mut total = 0;
    for (table, table_rows) in groups {
        total += insert_rows_mysql(&mut tx, &table, &table_rows, replace).await?;
    }

    sqlx::query("SET FOREIGN_KEY_CHECKS = 1")
        .execute(&mut *tx)
        .await
        .context("Failed to re-enable foreign key checks")?;
    tx.commit().await.context("Failed to commit import")?;
    Ok(total)
}

async fn insert_rows_mysql(
    conn: &mut sqlx::MySqlConnection,
    table: &str,
    rows: &[&serde_json::Map<String, serde_json::Value>],
    replace: bool,
) -> Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }

    let cols: Vec<&str> = rows[0]
        .keys()
        .filter(|k| *k != "__table")
        .map(|k| k.as_str())
        .collect();

    if cols.is_empty() {
        return Ok(0);
    }

    let uuid_cols = fetch_uuid_columns_mysql(&mut *conn, table).await?;
    let mut count = 0;

    for row in rows {
        let col_list = cols
            .iter()
            .map(|c| quote_ident_mysql(c))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = cols.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let conflict = if replace {
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
        } else {
            format!(
                "ON DUPLICATE KEY UPDATE {} = {}",
                quote_ident_mysql(cols[0]),
                quote_ident_mysql(cols[0])
            )
        };
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({}) {}",
            quote_ident_mysql(table),
            col_list,
            placeholders,
            conflict
        );

        let mut q = sqlx::query(&sql);
        for col in &cols {
            q = bind_json_value_mysql(
                q,
                row.get(*col).unwrap_or(&serde_json::Value::Null),
                col,
                uuid_cols.contains(*col),
            )?;
        }
        q.execute(&mut *conn)
            .await
            .with_context(|| format!("Failed to insert into {}", table))?;
        count += 1;
    }
    Ok(count)
}

async fn fetch_uuid_columns_mysql(
    conn: &mut sqlx::MySqlConnection,
    table: &str,
) -> Result<HashSet<String>> {
    use sqlx::Row;

    let rows = sqlx::query(
        "SELECT CAST(COLUMN_NAME AS CHAR) FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = DATABASE() \
           AND TABLE_NAME = ? \
           AND DATA_TYPE = 'binary' \
           AND CHARACTER_OCTET_LENGTH = 16",
    )
    .bind(table)
    .fetch_all(conn)
    .await
    .context("Failed to fetch UUID columns")?;

    Ok(rows
        .into_iter()
        .map(|r| r.try_get::<String, _>(0).unwrap_or_default())
        .collect())
}

fn bind_json_value_mysql<'q>(
    q: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    val: &serde_json::Value,
    column: &str,
    is_uuid_col: bool,
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
            } else {
                Ok(q.bind(s.clone()))
            }
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Ok(q.bind(val.to_string())),
    }
}

fn uuid_text_to_bytes(s: &str) -> Option<Vec<u8>> {
    if s.len() == 36 {
        let bytes = s.as_bytes();
        if bytes[8] != b'-' || bytes[13] != b'-' || bytes[18] != b'-' || bytes[23] != b'-' {
            return None;
        }
        let hex = s.replace('-', "");
        hex_to_bytes(&hex)
    } else if s.len() == 32 {
        hex_to_bytes(s)
    } else {
        None
    }
}

fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(hex.len() / 2);
    let mut chars = hex.chars();

    while let Some(hi) = chars.next() {
        let lo = chars.next()?;
        out.push(hex_pair_to_byte(hi, lo)?);
    }

    Some(out)
}

fn hex_pair_to_byte(hi: char, lo: char) -> Option<u8> {
    Some(hex_digit(hi)? * 16 + hex_digit(lo)?)
}

fn hex_digit(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'a'..='f' => Some(c as u8 - b'a' + 10),
        'A'..='F' => Some(c as u8 - b'A' + 10),
        _ => None,
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

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

/// Orders tables so that referenced (parent) tables come before the tables that
/// reference them. `edges` are `(child, parent)` pairs; edges touching tables not
/// in `tables`, and self-references, are ignored. Ties are broken alphabetically so
/// the order is deterministic.
fn order_by_dependencies(mut tables: Vec<String>, edges: &[(String, String)]) -> Vec<String> {
    tables.sort();
    tables.dedup();

    let table_set: HashSet<&str> = tables.iter().map(String::as_str).collect();

    let mut indegree: std::collections::HashMap<String, usize> =
        tables.iter().map(|t| (t.clone(), 0)).collect();
    let mut children: std::collections::HashMap<String, Vec<String>> =
        tables.iter().map(|t| (t.clone(), Vec::new())).collect();

    for (child, parent) in edges {
        if child == parent {
            continue;
        }
        if table_set.contains(child.as_str()) && table_set.contains(parent.as_str()) {
            if let Some(dependents) = children.get_mut(parent) {
                dependents.push(child.clone());
            }
            if let Some(deg) = indegree.get_mut(child) {
                *deg += 1;
            }
        }
    }

    let mut available: Vec<String> = tables
        .iter()
        .filter(|t| indegree.get(*t).copied().unwrap_or(0) == 0)
        .cloned()
        .collect();
    available.sort();

    let mut ordered = Vec::with_capacity(tables.len());
    while !available.is_empty() {
        let table = available.remove(0);
        ordered.push(table.clone());

        if let Some(dependents) = children.get(&table) {
            for dependent in dependents {
                if let Some(deg) = indegree.get_mut(dependent) {
                    *deg -= 1;
                    if *deg == 0 {
                        available.push(dependent.clone());
                    }
                }
            }
        }

        available.sort();
    }

    // ponytail: cycles between tables are inserted in alphabetical order and may
    // still fail; deferrable FKs would be the upgrade.
    let ordered_set: HashSet<String> = ordered.iter().cloned().collect();
    let mut remaining: Vec<String> = tables
        .iter()
        .filter(|t| !ordered_set.contains(*t))
        .cloned()
        .collect();
    remaining.sort();
    ordered.extend(remaining);

    ordered
}

/// Foreign keys between tables of the current schema, as `(child, parent)` pairs.
async fn fk_edges_postgres(conn: &mut sqlx::PgConnection) -> Result<Vec<(String, String)>> {
    let sql = "SELECT c.relname::text, p.relname::text
FROM pg_constraint k
JOIN pg_class c ON c.oid = k.conrelid
JOIN pg_class p ON p.oid = k.confrelid
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE k.contype = 'f' AND n.nspname = current_schema()";

    sqlx::query_as::<_, (String, String)>(sql)
        .fetch_all(&mut *conn)
        .await
        .context("Failed to read foreign keys")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_text_to_bytes_parses_hyphenated_uuid() {
        let bytes = uuid_text_to_bytes("123e4567-e89b-12d3-a456-426614174000").unwrap();
        assert_eq!(bytes.len(), 16);
        assert_eq!(bytes[0], 0x12);
        assert_eq!(bytes[15], 0x00);
    }

    #[test]
    fn uuid_text_to_bytes_parses_32_hex_uuid() {
        let bytes = uuid_text_to_bytes("123e4567e89b12d3a456426614174000").unwrap();
        assert_eq!(bytes.len(), 16);
        assert_eq!(bytes[0], 0x12);
        assert_eq!(bytes[15], 0x00);
    }

    #[test]
    fn uuid_text_to_bytes_rejects_invalid_uuids() {
        assert!(uuid_text_to_bytes("123e4567e89b12d3a45642661417400").is_none());
        assert!(uuid_text_to_bytes("123e4567-e89b-12d3-a456-42661417400g").is_none());
        assert!(uuid_text_to_bytes("123e4567-e89b-12d3-a456-4266141740000").is_none());
        assert!(uuid_text_to_bytes("123e4567e89b12d3a4564266141740000").is_none());
    }

    #[test]
    fn order_by_dependencies_puts_parents_first() {
        let tables = vec![
            "post".to_string(),
            "comment".to_string(),
            "user".to_string(),
        ];
        let edges = vec![
            ("post".to_string(), "user".to_string()),
            ("comment".to_string(), "post".to_string()),
            ("comment".to_string(), "user".to_string()),
        ];
        assert_eq!(
            order_by_dependencies(tables, &edges),
            vec![
                "user".to_string(),
                "post".to_string(),
                "comment".to_string()
            ]
        );
    }

    #[test]
    fn order_by_dependencies_ignores_self_refs_and_unknown_tables() {
        let tables = vec!["b".to_string(), "a".to_string()];
        let edges = vec![
            ("a".to_string(), "a".to_string()),
            ("b".to_string(), "zzz".to_string()),
        ];
        assert_eq!(
            order_by_dependencies(tables, &edges),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn order_by_dependencies_keeps_cycles() {
        let tables = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        let edges = vec![
            ("x".to_string(), "y".to_string()),
            ("y".to_string(), "x".to_string()),
            ("x".to_string(), "z".to_string()),
        ];
        let ordered = order_by_dependencies(tables, &edges);
        assert_eq!(ordered[0], "z");
        assert_eq!(ordered.len(), 3);
        assert!(ordered.contains(&"x".to_string()));
        assert!(ordered.contains(&"y".to_string()));
    }
}
