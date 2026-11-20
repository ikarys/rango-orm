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

/// RFC 4180 records as `(text, was_quoted)` fields: quoted fields may hold `,`, doubled `""`
/// and line breaks; records end at LF or CRLF; blank lines are skipped.
fn read_csv(content: &str) -> Result<Vec<Vec<(String, bool)>>> {
    let mut records = Vec::new();
    let mut record: Vec<(String, bool)> = Vec::new();
    let (mut field, mut quoted, mut in_quotes) = (String::new(), false, false);
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => in_quotes = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !quoted => (quoted, in_quotes) = (true, true),
            ',' => record.push((std::mem::take(&mut field), std::mem::take(&mut quoted))),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                record.push((std::mem::take(&mut field), std::mem::take(&mut quoted)));
                let done = std::mem::take(&mut record);
                if done.len() > 1 || !done[0].0.is_empty() || done[0].1 {
                    records.push(done);
                }
            }
            _ => field.push(c),
        }
    }
    if in_quotes {
        bail!("Invalid CSV: unterminated quoted field");
    }
    if !record.is_empty() || !field.is_empty() || quoted {
        record.push((field, quoted));
        records.push(record);
    }
    Ok(records)
}

pub(super) fn parse_csv(
    content: &str,
    table: Option<&str>,
) -> Result<Vec<serde_json::Map<String, serde_json::Value>>> {
    let mut records = read_csv(content)?.into_iter();
    let headers: Vec<String> = match records.next() {
        Some(h) => h.into_iter().map(|(h, _)| h.trim().to_string()).collect(),
        None => return Ok(vec![]),
    };

    Ok(records
        .map(|values| {
            let mut map = serde_json::Map::new();
            if let Some(t) = table {
                map.insert(
                    "__table".to_string(),
                    serde_json::Value::String(t.to_string()),
                );
            }
            for (i, header) in headers.iter().enumerate() {
                let (val, quoted) = values
                    .get(i)
                    .map(|(v, q)| (v.as_str(), *q))
                    .unwrap_or(("", false));
                let json_val = if quoted {
                    // A quoted field is always text (a JSON column value included).
                    serde_json::Value::String(val.to_string())
                } else if val.is_empty() {
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
                    serde_json::Value::String(val.to_string())
                };
                map.insert(header.clone(), json_val);
            }
            map
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_quoted_fields_crlf_and_blank_lines() {
        let csv = "a,b\r\n\"x,1\",\"say \"\"hi\"\"\"\r\n\r\n\"l1\nl2\",\n";
        assert_eq!(
            read_csv(csv).unwrap(),
            vec![
                vec![("a".into(), false), ("b".into(), false)],
                vec![("x,1".into(), true), ("say \"hi\"".into(), true)],
                vec![("l1\nl2".into(), true), ("".into(), false)],
            ]
        );
        assert!(read_csv("a\n\"open").is_err());
    }

    #[test]
    fn quoted_is_text_empty_is_null_unquoted_is_typed() {
        let csv = "id,s,e,n,j,t\n1,\"2\",\"\",,\"{\"\"a\"\":1}\",true\n";
        let rows = parse_csv(csv, Some("t")).unwrap();
        let r = &rows[0];
        assert_eq!(r["id"], json!(1));
        assert_eq!(r["s"], json!("2"));
        assert_eq!(r["e"], json!(""));
        assert_eq!(r["n"], json!(null));
        assert_eq!(r["j"], json!("{\"a\":1}"));
        assert_eq!(r["t"], json!(true));
        assert_eq!(r["__table"], json!("t"));
    }
}
