use anyhow::{bail, Context, Result};
use rango_core::BackendKind;
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
            "csv"  => Ok(Self::Csv),
            other  => bail!("Unknown format '{}'. Use 'json' or 'csv'.", other),
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
    let backend = cfg.database.backend_kind();

    let data = match backend {
        BackendKind::Sqlite => export_sqlite(database_url, table).await?,
        _                   => export_postgres(database_url, table).await?,
    };

    let content = match format {
        Format::Json => serialize_json(&data)?,
        Format::Csv  => serialize_csv(&data, table)?,
    };

    match output {
        Some(path) => {
            fs::write(path, &content)
                .with_context(|| format!("Failed to write to {}", path))?;
            println!("✅ Exported {} row(s) to {}", data.len(), path);
        }
        None => print!("{}", content),
    }

    Ok(())
}

// ─── Postgres export ──────────────────────────────────────────────────────────

async fn export_postgres(
    database_url: &str,
    table: Option<&str>,
) -> Result<Vec<serde_json::Map<String, serde_json::Value>>> {
    use sqlx::{Column, PgPool, Row};

    let pool = PgPool::connect(database_url).await
        .context("Failed to connect to Postgres")?;

    let tables = match table {
        Some(t) => vec![t.to_string()],
        None    => list_tables_postgres(&pool).await?,
    };

    let mut all_rows = Vec::new();
    for t in &tables {
        let sql = format!("SELECT * FROM \"{}\"", t);
        let rows = sqlx::query(&sql).fetch_all(&pool).await
            .with_context(|| format!("Failed to export table {}", t))?;

        for row in rows {
            let mut map = serde_json::Map::new();
            // Add table name for multi-table exports
            if table.is_none() {
                map.insert("__table".to_string(), serde_json::Value::String(t.clone()));
            }
            for (i, col) in row.columns().iter().enumerate() {
                let val = pg_column_to_json(&row, i);
                map.insert(col.name().to_string(), val);
            }
            all_rows.push(map);
        }
    }
    Ok(all_rows)
}

async fn list_tables_postgres(pool: &sqlx::PgPool) -> Result<Vec<String>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename"
    )
    .fetch_all(pool)
    .await
    .context("Failed to list tables")?;
    Ok(rows.into_iter()
        .map(|r| r.try_get::<String, _>(0).unwrap_or_default())
        .collect())
}

fn pg_column_to_json(row: &sqlx::postgres::PgRow, i: usize) -> serde_json::Value {
    use sqlx::{Column, Row, TypeInfo};
    let type_name = row.column(i).type_info().name().to_uppercase();
    match type_name.as_str() {
        "BOOL" => row.try_get::<Option<bool>, _>(i).ok()
            .map(|v| v.map(serde_json::Value::Bool).unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null),
        "INT2" => row.try_get::<Option<i16>, _>(i).ok()
            .map(|v| v.map(|n| serde_json::Value::Number((n as i64).into())).unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null),
        "INT4" | "SERIAL" => row.try_get::<Option<i32>, _>(i).ok()
            .map(|v| v.map(|n| serde_json::Value::Number((n as i64).into())).unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null),
        "INT8" | "BIGSERIAL" => row.try_get::<Option<i64>, _>(i).ok()
            .map(|v| v.map(|n| serde_json::Value::Number(n.into())).unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null),
        "FLOAT4" => row.try_get::<Option<f32>, _>(i).ok()
            .map(|v| v.and_then(|n| serde_json::Number::from_f64(n as f64))
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null),
        "FLOAT8" | "NUMERIC" => row.try_get::<Option<f64>, _>(i).ok()
            .map(|v| v.and_then(serde_json::Number::from_f64)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null),
        _ => row.try_get::<Option<String>, _>(i).ok()
            .map(|v| v.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null),
    }
}

// ─── SQLite export ────────────────────────────────────────────────────────────

async fn export_sqlite(
    database_url: &str,
    table: Option<&str>,
) -> Result<Vec<serde_json::Map<String, serde_json::Value>>> {
    use sqlx::{Column, SqlitePool, Row};

    let pool = SqlitePool::connect(database_url).await
        .context("Failed to connect to SQLite")?;

    let tables = match table {
        Some(t) => vec![t.to_string()],
        None    => list_tables_sqlite(&pool).await?,
    };

    let mut all_rows = Vec::new();
    for t in &tables {
        let sql = format!("SELECT * FROM \"{}\"", t);
        let rows = sqlx::query(&sql).fetch_all(&pool).await
            .with_context(|| format!("Failed to export table {}", t))?;

        for row in rows {
            let mut map = serde_json::Map::new();
            if table.is_none() {
                map.insert("__table".to_string(), serde_json::Value::String(t.clone()));
            }
            for (i, col) in row.columns().iter().enumerate() {
                let val = sqlite_column_to_json(&row, i);
                map.insert(col.name().to_string(), val);
            }
            all_rows.push(map);
        }
    }
    Ok(all_rows)
}

async fn list_tables_sqlite(pool: &sqlx::SqlitePool) -> Result<Vec<String>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
    )
    .fetch_all(pool)
    .await
    .context("Failed to list tables")?;
    Ok(rows.into_iter()
        .map(|r| r.try_get::<String, _>(0).unwrap_or_default())
        .collect())
}

fn sqlite_column_to_json(row: &sqlx::sqlite::SqliteRow, i: usize) -> serde_json::Value {
    use sqlx::Row;
    if let Ok(v) = row.try_get::<Option<i64>, _>(i) {
        return v.map(|n| serde_json::Value::Number(n.into())).unwrap_or(serde_json::Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<f64>, _>(i) {
        return v.and_then(serde_json::Number::from_f64)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<String>, _>(i) {
        return v.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null);
    }
    serde_json::Value::Null
}

// ─── Serializers ──────────────────────────────────────────────────────────────

fn serialize_json(rows: &[serde_json::Map<String, serde_json::Value>]) -> Result<String> {
    serde_json::to_string_pretty(rows)
        .context("Failed to serialize JSON")
}

fn serialize_csv(
    rows: &[serde_json::Map<String, serde_json::Value>],
    _table: Option<&str>,
) -> Result<String> {
    if rows.is_empty() {
        return Ok(String::new());
    }

    let headers: Vec<String> = rows[0].keys()
        .filter(|k| *k != "__table")
        .cloned()
        .collect();

    let mut out = headers.join(",");
    out.push('\n');

    for row in rows {
        let values: Vec<String> = headers.iter().map(|h| {
            match row.get(h) {
                Some(serde_json::Value::String(s)) => {
                    // Quote strings containing commas or newlines
                    if s.contains(',') || s.contains('\n') || s.contains('"') {
                        format!("\"{}\"", s.replace('"', "\"\""))
                    } else {
                        s.clone()
                    }
                }
                Some(serde_json::Value::Null) => String::new(),
                Some(v) => v.to_string(),
                None => String::new(),
            }
        }).collect();
        out.push_str(&values.join(","));
        out.push('\n');
    }

    Ok(out)
}
