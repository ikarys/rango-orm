//! JSON and CSV input parsers.

use anyhow::{Context, Result, bail};

pub(super) fn parse_json(content: &str) -> Result<Vec<serde_json::Map<String, serde_json::Value>>> {
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

pub(super) fn parse_csv(
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
