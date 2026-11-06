use anyhow::{Context, Result, bail};
use rango_orm_core::{BackendKind, quote_ident, quote_ident_mysql};
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Clone, Copy)]
pub enum Format {
    Json,
    Csv,
}

impl Format {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "json" => Ok(Self::Json),
            "csv" => Ok(Self::Csv),
            other => bail!("Unknown format '{}'. Use 'json' or 'csv'.", other),
        }
    }
}

pub async fn run(
    database_url: &str,
    table: Option<&str>,
    format: Format,
    output: Option<&str>,
) -> Result<()> {
    let cfg = crate::config::RangoConfig::load().unwrap_or_default();
    let requested: Vec<String> = table.map(str::to_string).into_iter().collect();

    let mut data = Vec::new();
    for (name, rows) in fetch_tables(cfg.database.backend_kind(), database_url, &requested).await? {
        for mut row in rows {
            // Add table name for multi-table exports
            if table.is_none() {
                row.insert(
                    "__table".to_string(),
                    serde_json::Value::String(name.clone()),
                );
            }
            data.push(row);
        }
    }

    let content = match format {
        Format::Json => serialize_json(&data)?,
        Format::Csv => serialize_csv(&data, table)?,
    };

    match output {
        Some(path) => {
            fs::write(path, &content).with_context(|| format!("Failed to write to {}", path))?;
            println!("✅ Exported {} row(s) to {}", data.len(), path);
        }
        None => print!("{}", content),
    }

    Ok(())
}

pub(crate) type Rows = Vec<serde_json::Map<String, serde_json::Value>>;

/// Rows of `requested` tables (all tables when empty), in foreign-key order: parents first.
pub(crate) async fn fetch_tables(
    backend: BackendKind,
    database_url: &str,
    requested: &[String],
) -> Result<Vec<(String, Rows)>> {
    match backend {
        BackendKind::Sqlite => export_sqlite(database_url, requested).await,
        BackendKind::Mysql => export_mysql(database_url, requested).await,
        _ => export_postgres(database_url, requested).await,
    }
}

/// `requested` (checked against `existing`) or every table, ordered parents first.
fn select_tables(
    existing: Vec<String>,
    requested: &[String],
    edges: &[(String, String)],
) -> Result<Vec<String>> {
    for t in requested {
        if !existing.contains(t) {
            bail!("Unknown table '{}'", t);
        }
    }
    let wanted = if requested.is_empty() {
        existing
    } else {
        requested.to_vec()
    };
    Ok(super::import::order_by_dependencies(wanted, edges))
}

// ─── Postgres export ──────────────────────────────────────────────────────────

async fn export_postgres(database_url: &str, requested: &[String]) -> Result<Vec<(String, Rows)>> {
    let pool = sqlx::PgPool::connect(database_url)
        .await
        .context("Failed to connect to Postgres")?;

    let mut conn = pool
        .acquire()
        .await
        .context("Failed to acquire a connection")?;
    let edges = super::import::fk_edges_postgres(&mut conn).await?;
    let tables = select_tables(list_tables_postgres(&pool).await?, requested, &edges)?;

    let mut out = Vec::new();
    for t in tables {
        // to_jsonb renders every column type (uuid, timestamps, numeric, ...) as JSON,
        // and the import side casts it back with jsonb_populate_record.
        let sql = format!("SELECT to_jsonb(t)::text FROM {} t", quote_ident(&t));
        let texts: Vec<String> = sqlx::query_scalar(&sql)
            .fetch_all(&pool)
            .await
            .with_context(|| format!("Failed to export table {}", t))?;
        let mut rows = Vec::with_capacity(texts.len());
        for text in texts {
            match serde_json::from_str(&text)? {
                serde_json::Value::Object(map) => rows.push(map),
                _ => bail!("Unexpected row shape in table {}", t),
            }
        }
        out.push((t, rows));
    }
    Ok(out)
}

async fn list_tables_postgres(pool: &sqlx::PgPool) -> Result<Vec<String>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename",
    )
    .fetch_all(pool)
    .await
    .context("Failed to list tables")?;
    Ok(rows
        .into_iter()
        .map(|r| r.try_get::<String, _>(0).unwrap_or_default())
        .collect())
}

// ─── SQLite export ────────────────────────────────────────────────────────────

async fn export_sqlite(database_url: &str, requested: &[String]) -> Result<Vec<(String, Rows)>> {
    use sqlx::{Column, Row, SqlitePool};

    let pool = SqlitePool::connect(database_url)
        .await
        .context("Failed to connect to SQLite")?;

    let edges: Vec<(String, String)> = sqlx::query_as(
        "SELECT m.name, f.\"table\" FROM sqlite_master m, pragma_foreign_key_list(m.name) f \
         WHERE m.type = 'table'",
    )
    .fetch_all(&pool)
    .await
    .context("Failed to read foreign keys")?;
    let tables = select_tables(list_tables_sqlite(&pool).await?, requested, &edges)?;

    let mut out = Vec::new();
    for t in tables {
        let sql = format!("SELECT * FROM {}", quote_ident(&t));
        let fetched = sqlx::query(&sql)
            .fetch_all(&pool)
            .await
            .with_context(|| format!("Failed to export table {}", t))?;

        let mut rows = Vec::with_capacity(fetched.len());
        for row in fetched {
            let mut map = serde_json::Map::new();
            for (i, col) in row.columns().iter().enumerate() {
                map.insert(col.name().to_string(), sqlite_column_to_json(&row, i));
            }
            rows.push(map);
        }
        out.push((t, rows));
    }
    Ok(out)
}

async fn list_tables_sqlite(pool: &sqlx::SqlitePool) -> Result<Vec<String>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
    )
    .fetch_all(pool)
    .await
    .context("Failed to list tables")?;
    Ok(rows
        .into_iter()
        .map(|r| r.try_get::<String, _>(0).unwrap_or_default())
        .collect())
}

fn sqlite_column_to_json(row: &sqlx::sqlite::SqliteRow, i: usize) -> serde_json::Value {
    use sqlx::Row;
    if let Ok(v) = row.try_get::<Option<i64>, _>(i) {
        return v
            .map(|n| serde_json::Value::Number(n.into()))
            .unwrap_or(serde_json::Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<f64>, _>(i) {
        return v
            .and_then(serde_json::Number::from_f64)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<String>, _>(i) {
        return v
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null);
    }
    serde_json::Value::Null
}

// ─── MySQL export ─────────────────────────────────────────────────────────────

async fn export_mysql(database_url: &str, requested: &[String]) -> Result<Vec<(String, Rows)>> {
    use sqlx::{Column, Row};

    let pool = rango_orm_mysql::connect(database_url)
        .await
        .context("Failed to connect to MySQL")?;

    let edges: Vec<(String, String)> = sqlx::query_as(
        "SELECT CAST(TABLE_NAME AS CHAR), CAST(REFERENCED_TABLE_NAME AS CHAR) \
         FROM information_schema.KEY_COLUMN_USAGE \
         WHERE TABLE_SCHEMA = DATABASE() AND REFERENCED_TABLE_NAME IS NOT NULL",
    )
    .fetch_all(&pool)
    .await
    .context("Failed to read foreign keys")?;
    let tables = select_tables(list_tables_mysql(&pool).await?, requested, &edges)?;

    let mut out = Vec::new();
    for t in tables {
        let sql = format!("SELECT * FROM {}", quote_ident_mysql(&t));
        let fetched = sqlx::raw_sql(&sql)
            .fetch_all(&pool)
            .await
            .with_context(|| format!("Failed to export table {}", t))?;
        let kinds = text_column_kinds_mysql(&pool, &t).await?;

        let mut rows = Vec::with_capacity(fetched.len());
        for row in fetched {
            let mut map = serde_json::Map::new();
            for (i, col) in row.columns().iter().enumerate() {
                let val = mysql_column_to_json(&row, i, kinds.get(col.name()).copied());
                map.insert(col.name().to_string(), val);
            }
            rows.push(map);
        }
        out.push((t, rows));
    }
    Ok(out)
}

async fn list_tables_mysql(pool: &sqlx::MySqlPool) -> Result<Vec<String>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT CAST(TABLE_NAME AS CHAR) FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_TYPE = 'BASE TABLE' \
         ORDER BY TABLE_NAME",
    )
    .fetch_all(pool)
    .await
    .context("Failed to list tables")?;
    Ok(rows
        .into_iter()
        .map(|r| r.try_get::<String, _>(0).unwrap_or_default())
        .collect())
}

#[derive(Clone, Copy, PartialEq)]
enum TextKind {
    Text,
    Json,
}

/// Character columns of `table`, flagged JSON when the server says so.
///
/// sqlx reports any column with the BINARY flag as a blob, which includes
/// `utf8mb4_bin` text and MariaDB's JSON (a `LONGTEXT` with a `json_valid` check),
/// so the catalog decides instead.
async fn text_column_kinds_mysql(
    pool: &sqlx::MySqlPool,
    table: &str,
) -> Result<HashMap<String, TextKind>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT CAST(c.COLUMN_NAME AS CHAR), \
           c.DATA_TYPE = 'json' OR EXISTS ( \
             SELECT 1 FROM information_schema.TABLE_CONSTRAINTS tc \
             JOIN information_schema.CHECK_CONSTRAINTS cc \
               ON cc.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA \
              AND cc.CONSTRAINT_NAME = tc.CONSTRAINT_NAME \
             WHERE tc.TABLE_SCHEMA = c.TABLE_SCHEMA AND tc.TABLE_NAME = c.TABLE_NAME \
               AND tc.CONSTRAINT_TYPE = 'CHECK' \
               AND cc.CHECK_CLAUSE = CONCAT('json_valid(`', c.COLUMN_NAME, '`)')) \
         FROM information_schema.COLUMNS c \
         WHERE c.TABLE_SCHEMA = DATABASE() AND c.TABLE_NAME = ? \
           AND (c.CHARACTER_SET_NAME IS NOT NULL OR c.DATA_TYPE = 'json')",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .with_context(|| format!("Failed to read column types of {}", table))?;

    let mut kinds = HashMap::new();
    for r in rows {
        let name: String = r.try_get(0)?;
        let is_json: i64 = r.try_get(1)?;
        let kind = if is_json != 0 {
            TextKind::Json
        } else {
            TextKind::Text
        };
        kinds.insert(name, kind);
    }
    Ok(kinds)
}

fn mysql_column_to_json(
    row: &sqlx::mysql::MySqlRow,
    i: usize,
    text_kind: Option<TextKind>,
) -> serde_json::Value {
    use sqlx::{Column, Row, TypeInfo};

    let bytes = match row.try_get_unchecked::<Option<Vec<u8>>, _>(i) {
        Ok(Some(bytes)) => bytes,
        _ => return serde_json::Value::Null,
    };

    let type_name = row.column(i).type_info().name().to_uppercase();

    if matches!(
        type_name.as_str(),
        "BOOLEAN" | "TINYINT" | "TINYINT UNSIGNED"
    ) {
        return parse_i64_bytes(&bytes)
            .map(|n| {
                if type_name == "BOOLEAN" {
                    serde_json::Value::Bool(n != 0)
                } else {
                    serde_json::Value::Number(n.into())
                }
            })
            .unwrap_or(serde_json::Value::Null);
    }

    if is_integer_type(&type_name) {
        return parse_i64_bytes(&bytes)
            .map(serde_json::Number::from)
            .or_else(|| parse_u64_bytes(&bytes).map(serde_json::Number::from))
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null);
    }

    if matches!(type_name.as_str(), "FLOAT" | "DOUBLE" | "DECIMAL") {
        return parse_f64_bytes(&bytes)
            .and_then(serde_json::Number::from_f64)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null);
    }

    if type_name == "JSON" || text_kind == Some(TextKind::Json) {
        return serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_else(|_| {
            serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned())
        });
    }

    if is_binary_type(&type_name) && text_kind.is_none() {
        if matches!(type_name.as_str(), "BINARY" | "VARBINARY") && bytes.len() == 16 {
            return serde_json::Value::String(bytes_to_uuid_string(&bytes));
        }
        return serde_json::Value::String(hex_prefixed(&bytes));
    }

    serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned())
}

fn is_integer_type(name: &str) -> bool {
    matches!(
        name,
        "SMALLINT"
            | "MEDIUMINT"
            | "INT"
            | "BIGINT"
            | "SMALLINT UNSIGNED"
            | "MEDIUMINT UNSIGNED"
            | "INT UNSIGNED"
            | "BIGINT UNSIGNED"
            | "YEAR"
    )
}

fn is_binary_type(name: &str) -> bool {
    matches!(
        name,
        "BINARY" | "VARBINARY" | "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BIT"
    )
}

fn parse_i64_bytes(bytes: &[u8]) -> Option<i64> {
    std::str::from_utf8(bytes).ok()?.parse::<i64>().ok()
}

fn parse_u64_bytes(bytes: &[u8]) -> Option<u64> {
    std::str::from_utf8(bytes).ok()?.parse::<u64>().ok()
}

fn parse_f64_bytes(bytes: &[u8]) -> Option<f64> {
    std::str::from_utf8(bytes).ok()?.parse::<f64>().ok()
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

fn push_hex(out: &mut String, byte: u8) {
    out.push(char::from(HEX_DIGITS[(byte >> 4) as usize]));
    out.push(char::from(HEX_DIGITS[(byte & 0x0f) as usize]));
}

fn bytes_to_uuid_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(36);
    for (i, byte) in bytes.iter().enumerate() {
        if i == 4 || i == 6 || i == 8 || i == 10 {
            out.push('-');
        }
        push_hex(&mut out, *byte);
    }
    out
}

fn hex_prefixed(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(2 + bytes.len() * 2);
    out.push_str("\\x");
    for byte in bytes {
        push_hex(&mut out, *byte);
    }
    out
}

// ─── Serializers ──────────────────────────────────────────────────────────────

fn serialize_json(rows: &[serde_json::Map<String, serde_json::Value>]) -> Result<String> {
    serde_json::to_string_pretty(rows).context("Failed to serialize JSON")
}

fn serialize_csv(
    rows: &[serde_json::Map<String, serde_json::Value>],
    _table: Option<&str>,
) -> Result<String> {
    if rows.is_empty() {
        return Ok(String::new());
    }

    let headers: Vec<String> = rows[0]
        .keys()
        .filter(|k| *k != "__table")
        .cloned()
        .collect();

    let mut out = headers.join(",");
    out.push('\n');

    for row in rows {
        let values: Vec<String> = headers
            .iter()
            .map(|h| {
                match row.get(h) {
                    Some(serde_json::Value::String(s)) => {
                        // Quote strings containing commas or newlines
                        if s.contains(',')
                            || s.contains('\n')
                            || s.contains('"')
                            || s.contains('\r')
                        {
                            format!("\"{}\"", s.replace('"', "\"\""))
                        } else {
                            s.clone()
                        }
                    }
                    Some(serde_json::Value::Null) => String::new(),
                    Some(v) => v.to_string(),
                    None => String::new(),
                }
            })
            .collect();
        out.push_str(&values.join(","));
        out.push('\n');
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_to_uuid_string_formats_canonical_lowercase() {
        let bytes = [
            0x12, 0x3e, 0x45, 0x67, 0xe8, 0x9b, 0x12, 0xd3, 0xa4, 0x56, 0x42, 0x66, 0x14, 0x17,
            0x40, 0x00,
        ];
        assert_eq!(
            bytes_to_uuid_string(&bytes),
            "123e4567-e89b-12d3-a456-426614174000"
        );
    }
}
