use anyhow::{Context, Result, bail};
use rango_core::{ColumnType, DefaultValue, TableSchema};
use std::fs;

use crate::scanner::scan_models;

pub fn run(src_dir: &str, output_dir: &str, prefix: Option<&str>) -> Result<()> {
    println!("🔍 Scanning models in {}...", src_dir);

    let prefix = match prefix {
        Some(p) => p.to_string(),
        None => detect_project_name()?,
    };
    println!("📦 Table prefix: {}", prefix);

    let mut schemas = scan_models(src_dir)?;

    // Apply prefix to all table names (skip if table was explicitly set via #[model(table="...")])
    for schema in &mut schemas {
        schema.table_name = format!("{}_{}", prefix, schema.table_name);
    }

    if schemas.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    println!("Found {} model(s):", schemas.len());
    for s in &schemas {
        println!("  - {} ({} columns)", s.table_name, s.columns.len());
    }

    fs::create_dir_all(output_dir)?;

    let next_num = next_migration_number(output_dir)?;
    let label = migration_label(&schemas);
    let filename = format!("{}/{:04}_{}.sql", output_dir, next_num, label);

    let sql = generate_sql(&schemas);
    fs::write(&filename, &sql)?;

    println!("✅ Migration generated: {}", filename);
    Ok(())
}

/// Auto-detect project name from Cargo.toml in current directory
fn detect_project_name() -> Result<String> {
    let cargo_toml = fs::read_to_string("Cargo.toml")
        .context("Could not find Cargo.toml — run from project root or use --prefix")?;

    for line in cargo_toml.lines() {
        let line = line.trim();
        if line.starts_with("name") {
            if let Some(val) = line.splitn(2, '=').nth(1) {
                let name = val.trim().trim_matches('"').to_string();
                return Ok(name);
            }
        }
    }

    anyhow::bail!("Could not detect project name from Cargo.toml — use --prefix")
}

/// Build a readable label from table names — truncated if too many
fn migration_label(schemas: &[rango_core::TableSchema]) -> String {
    let names: Vec<&str> = schemas.iter().map(|s| s.table_name.as_str()).collect();
    let joined = names.join("_");
    if joined.len() <= 40 {
        joined
    } else {
        "auto".to_string()
    }
}

/// Find next available migration number
fn next_migration_number(dir: &str) -> Result<u32> {
    let mut max = 0u32;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(num_str) = name.split('_').next() {
                if let Ok(n) = num_str.parse::<u32>() {
                    max = max.max(n);
                }
            }
        }
    }
    Ok(max + 1)
}

/// Generate SQL DDL for a list of schemas
fn generate_sql(schemas: &[TableSchema]) -> String {
    let mut sql = String::new();
    for schema in schemas {
        sql.push_str(&generate_create_table(schema));
        sql.push('\n');
    }
    sql
}

fn generate_create_table(schema: &TableSchema) -> String {
    let mut lines = Vec::new();

    for col in &schema.columns {
        let mut def = format!("    {} {}", col.name, sql_type(&col.col_type));

        if col.primary_key {
            def.push_str(" PRIMARY KEY");
        } else {
            if !col.nullable {
                def.push_str(" NOT NULL");
            }
            if col.unique {
                def.push_str(" UNIQUE");
            }
        }

        if let Some(default) = &col.default {
            match default {
                DefaultValue::CurrentTimestamp => def.push_str(" DEFAULT now()"),
                DefaultValue::Literal(v) => def.push_str(&format!(" DEFAULT {}", v)),
                DefaultValue::GeneratedUuid => def.push_str(" DEFAULT gen_random_uuid()"),
            }
        }

        lines.push(def);
    }

    format!(
        "CREATE TABLE IF NOT EXISTS \"{}\" (\n{}\n);\n",
        schema.table_name,
        lines.join(",\n")
    )
}

fn sql_type(col_type: &ColumnType) -> &'static str {
    match col_type {
        ColumnType::Bool           => "BOOLEAN",
        ColumnType::SmallInt       => "SMALLINT",
        ColumnType::Int            => "INTEGER",
        ColumnType::BigInt         => "BIGINT",
        ColumnType::Float          => "REAL",
        ColumnType::Double         => "DOUBLE PRECISION",
        ColumnType::Text           => "TEXT",
        ColumnType::Bytea          => "BYTEA",
        ColumnType::Uuid           => "UUID",
        ColumnType::Date           => "DATE",
        ColumnType::Time           => "TIME",
        ColumnType::DateTime       => "TIMESTAMPTZ",
        ColumnType::Json           => "JSON",
        ColumnType::Jsonb          => "JSONB",
        ColumnType::Varchar(_)     => "VARCHAR",
        ColumnType::Decimal { .. } => "NUMERIC",
    }
}
