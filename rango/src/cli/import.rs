use anyhow::{bail, Context, Result};
use rango_core::BackendKind;
use std::fs;

pub async fn run(
    database_url: &str,
    input: &str,
    table: Option<&str>,
    format: Option<&str>,
) -> Result<()> {
    let cfg = crate::config::RangoConfig::load().unwrap_or_default();
    let backend = cfg.database.backend_kind();

    let content = fs::read_to_string(input)
        .with_context(|| format!("Failed to read {}", input))?;

    // Auto-detect format from extension if not specified
    let fmt = match format {
        Some(f) => f.to_string(),
        None => {
            if input.ends_with(".json") { "json".to_string() }
            else if input.ends_with(".csv") { "csv".to_string() }
            else { bail!("Cannot detect format from filename '{}'. Use --format json|csv.", input) }
        }
    };

    let rows = match fmt.as_str() {
        "json" => parse_json(&content)?,
        "csv"  => parse_csv(&content, table)?,
        other  => bail!("Unknown format '{}'. Use 'json' or 'csv'.", other),
    };

    if rows.is_empty() {
        println!("No rows to import.");
        return Ok(());
    }

    let count = match backend {
        BackendKind::Sqlite => import_sqlite(database_url, &rows, table).await?,
        _                   => import_postgres(database_url, &rows, table).await?,
    };

    println!("✅ Imported {} row(s) from {}", count, input);
    Ok(())
}

// ─── Parsers ──────────────────────────────────────────────────────────────────

fn parse_json(content: &str) -> Result<Vec<serde_json::Map<String, serde_json::Value>>> {
    let value: serde_json::Value = serde_json::from_str(content)
        .context("Failed to parse JSON")?;

    match value {
        serde_json::Value::Array(arr) => arr.into_iter()
            .map(|v| match v {
                serde_json::Value::Object(map) => Ok(map),
                _ => bail!("Expected array of objects"),
            })
            .collect(),
        serde_json::Value::Object(map) => Ok(vec![map]),
        _ => bail!("Expected JSON array or object"),
    }
}

fn parse_csv(content: &str, table: Option<&str>) -> Result<Vec<serde_json::Map<String, serde_json::Value>>> {
    let mut lines = content.lines();
    let headers: Vec<&str> = match lines.next() {
        Some(h) => h.split(',').collect(),
        None => return Ok(vec![]),
    };

    lines.map(|line| {
        let values: Vec<&str> = line.splitn(headers.len(), ',').collect();
        let mut map = serde_json::Map::new();
        if let Some(t) = table {
            map.insert("__table".to_string(), serde_json::Value::String(t.to_string()));
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
                    val[1..val.len()-1].replace("\"\"", "\"")
                } else {
                    val.to_string()
                };
                serde_json::Value::String(s)
            };
            map.insert(header.trim().to_string(), json_val);
        }
        Ok(map)
    }).collect()
}

// ─── Postgres import ──────────────────────────────────────────────────────────

async fn import_postgres(
    database_url: &str,
    rows: &[serde_json::Map<String, serde_json::Value>],
    table: Option<&str>,
) -> Result<usize> {
    let pool = sqlx::PgPool::connect(database_url).await
        .context("Failed to connect to Postgres")?;

    // Group rows by table
    let groups = group_by_table(rows, table);
    let mut total = 0;

    for (table, table_rows) in groups {
        total += insert_rows_postgres(&pool, &table, &table_rows).await?;
    }
    Ok(total)
}

async fn insert_rows_postgres(
    pool: &sqlx::PgPool,
    table: &str,
    rows: &[&serde_json::Map<String, serde_json::Value>],
) -> Result<usize> {
    if rows.is_empty() { return Ok(0); }

    let cols: Vec<&str> = rows[0].keys()
        .filter(|k| *k != "__table")
        .map(|k| k.as_str())
        .collect();

    let mut count = 0;
    for row in rows {
        let col_list = cols.iter().map(|c| format!("\"{}\"", c)).collect::<Vec<_>>().join(", ");
        let placeholders = (1..=cols.len()).map(|i| format!("${}", i)).collect::<Vec<_>>().join(", ");
        let sql = format!("INSERT INTO \"{}\" ({}) VALUES ({}) ON CONFLICT DO NOTHING", table, col_list, placeholders);

        let mut q = sqlx::query(&sql);
        for col in &cols {
            q = bind_json_value_pg(q, row.get(*col).unwrap_or(&serde_json::Value::Null));
        }
        q.execute(pool).await
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
        serde_json::Value::Null           => q.bind(Option::<i64>::None),
        serde_json::Value::Bool(b)        => q.bind(*b as i64),
        serde_json::Value::Number(n)      => {
            if let Some(i) = n.as_i64() { q.bind(i) }
            else if let Some(f) = n.as_f64() { q.bind(f) }
            else { q.bind(Option::<String>::None) }
        }
        serde_json::Value::String(s)      => q.bind(s.clone()),
        serde_json::Value::Array(_) |
        serde_json::Value::Object(_)      => q.bind(val.to_string()),
    }
}

// ─── SQLite import ────────────────────────────────────────────────────────────

async fn import_sqlite(
    database_url: &str,
    rows: &[serde_json::Map<String, serde_json::Value>],
    table: Option<&str>,
) -> Result<usize> {
    let pool = sqlx::SqlitePool::connect(database_url).await
        .context("Failed to connect to SQLite")?;

    let groups = group_by_table(rows, table);
    let mut total = 0;

    for (table, table_rows) in groups {
        total += insert_rows_sqlite(&pool, &table, &table_rows).await?;
    }
    Ok(total)
}

async fn insert_rows_sqlite(
    pool: &sqlx::SqlitePool,
    table: &str,
    rows: &[&serde_json::Map<String, serde_json::Value>],
) -> Result<usize> {
    if rows.is_empty() { return Ok(0); }

    let cols: Vec<&str> = rows[0].keys()
        .filter(|k| *k != "__table")
        .map(|k| k.as_str())
        .collect();

    let mut count = 0;
    for row in rows {
        let col_list = cols.iter().map(|c| format!("\"{}\"", c)).collect::<Vec<_>>().join(", ");
        let placeholders = cols.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = format!("INSERT OR IGNORE INTO \"{}\" ({}) VALUES ({})", table, col_list, placeholders);

        let mut q = sqlx::query(&sql);
        for col in &cols {
            q = bind_json_value_sqlite(q, row.get(*col).unwrap_or(&serde_json::Value::Null));
        }
        q.execute(pool).await
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
        serde_json::Value::Null           => q.bind(Option::<String>::None),
        serde_json::Value::Bool(b)        => q.bind(*b as i64),
        serde_json::Value::Number(n)      => {
            if let Some(i) = n.as_i64() { q.bind(i) }
            else if let Some(f) = n.as_f64() { q.bind(f) }
            else { q.bind(Option::<String>::None) }
        }
        serde_json::Value::String(s)      => q.bind(s.clone()),
        serde_json::Value::Array(_) |
        serde_json::Value::Object(_)      => q.bind(val.to_string()),
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Group rows by their `__table` field (or "default" if not present).
fn group_by_table<'a>(
    rows: &'a [serde_json::Map<String, serde_json::Value>],
    default_table: Option<&str>,
) -> std::collections::HashMap<String, Vec<&'a serde_json::Map<String, serde_json::Value>>> {
    let mut groups: std::collections::HashMap<String, Vec<&serde_json::Map<String, serde_json::Value>>> =
        std::collections::HashMap::new();

    for row in rows {
        let table = row.get("__table")
            .and_then(|v| v.as_str())
            .or(default_table)
            .unwrap_or("default")
            .to_string();
        groups.entry(table).or_default().push(row);
    }
    groups
}
