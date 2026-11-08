use crate::commands::migrate::tracking_table_ddl;
use crate::commands::{export, import};
use crate::migration::{diff::SchemaDiff, snapshot::Snapshot, sql::generate_sql_from_diff};
use anyhow::{Context, Result, bail};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use rango_orm_core::BackendKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::io::{Read, Write};
use std::path::Path;

const FORMAT: &str = "rango-dump";
/// Version 2: binary columns are base64 (version 1 wrote `\x` hex, still read).
const VERSION: u32 = 2;
/// Upper bound on a decompressed dump, against gzip bombs.
const MAX_DUMP_BYTES: u64 = 8 << 30;
/// Rango's own bookkeeping table: never part of a dump unless asked for by name.
const MIGRATIONS_TABLE: &str = "_rango_migrations";

#[derive(Serialize, Deserialize)]
struct Dump {
    format: String,
    version: u32,
    rango_version: String,
    backend: String,
    created_at: String,
    schema: Option<Snapshot>,
    /// Applied migration names. Absent in dumps made before this field existed.
    #[serde(default)]
    migrations: Option<Vec<String>>,
    tables: Vec<DumpTable>,
}

#[derive(Serialize, Deserialize)]
struct DumpTable {
    name: String,
    rows: export::Rows,
}

/// Applied migration names. Ids and timestamps are regenerated on restore.
fn migration_names(rows: &export::Rows) -> Vec<String> {
    rows.iter()
        .filter_map(|r| r.get("name")?.as_str().map(String::from))
        .collect()
}

/// Build a dump of the database (data) and the latest migration snapshot (schema).
async fn build_dump(
    database_url: &str,
    requested: &[String],
    with_schema: bool,
    with_data: bool,
    migrations_dir: &str,
) -> Result<Dump> {
    let cfg = crate::config::RangoConfig::load().unwrap_or_default();
    let backend = cfg.database.backend_kind();

    let mut schema = None;
    if with_schema {
        let snapshot = Snapshot::load(migrations_dir)?;
        if !snapshot.tables.is_empty() {
            schema = Some(snapshot);
        } else if !with_data {
            bail!("No schema snapshot found in {}", migrations_dir);
        }
    }

    let mut tables = Vec::new();
    let mut migrations = None;
    if with_data {
        for (name, rows) in export::fetch_tables(backend, database_url, requested).await? {
            if name != MIGRATIONS_TABLE || requested.contains(&name) {
                tables.push(DumpTable { name, rows });
            } else {
                migrations = Some(migration_names(&rows));
            }
        }
        // A `--table` dump still restores the whole schema, so it carries the ledger too.
        // The table is absent on a database `rango migrate` never ran against: no ledger then.
        if migrations.is_none() && !requested.is_empty() {
            let ledger = [MIGRATIONS_TABLE.to_string()];
            if let Ok(found) = export::fetch_tables(backend, database_url, &ledger).await {
                migrations = found.first().map(|(_, rows)| migration_names(rows));
            }
        }
    }

    Ok(Dump {
        format: FORMAT.to_string(),
        version: VERSION,
        rango_version: env!("CARGO_PKG_VERSION").to_string(),
        backend: match backend {
            BackendKind::Sqlite => "sqlite",
            BackendKind::Mysql => "mysql",
            _ => "postgres",
        }
        .to_string(),
        created_at: utc_now().0,
        schema,
        migrations,
        tables,
    })
}

pub async fn dump(
    database_url: &str,
    tables: &[String],
    schema_only: bool,
    data_only: bool,
    migrations_dir: &str,
    output: Option<&str>,
) -> Result<()> {
    let dump = build_dump(
        database_url,
        tables,
        !data_only,
        !schema_only,
        migrations_dir,
    )
    .await?;
    let json = serde_json::to_vec_pretty(&dump).context("Failed to serialize the dump")?;
    match output {
        Some(path) => {
            write_private(Path::new(path), &json)?;
            println!("✅ Dumped {} table(s) to {}", dump.tables.len(), path);
        }
        None => println!("{}", String::from_utf8_lossy(&json)),
    }
    Ok(())
}

pub async fn backup(database_url: &str, dir: &str, migrations_dir: &str) -> Result<()> {
    let dump = build_dump(database_url, &[], true, true, migrations_dir).await?;
    let json = serde_json::to_vec(&dump).context("Failed to serialize the dump")?;
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(&json)?;
    let bytes = gz.finish()?;

    std::fs::create_dir_all(dir).with_context(|| format!("Failed to create {}", dir))?;
    let path = Path::new(dir).join(format!("rango-backup-{}.json.gz", utc_now().1));
    write_private(&path, &bytes)?;
    println!("✅ Backup written to {}", path.display());
    Ok(())
}

pub async fn restore(database_url: &str, input: &str) -> Result<()> {
    let cfg = crate::config::RangoConfig::load().unwrap_or_default();
    let backend = cfg.database.backend_kind();

    let mut bytes = std::fs::read(input).with_context(|| format!("Failed to read {}", input))?;
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut plain = Vec::new();
        GzDecoder::new(bytes.as_slice())
            .take(MAX_DUMP_BYTES)
            .read_to_end(&mut plain)
            .context("Failed to decompress the dump")?;
        bytes = plain;
    }
    let dump = parse_dump(&bytes)?;

    // File contents are untrusted: names are checked against the dump schema here,
    // then quoted and values bound at insert time.
    validate(&dump)?;

    let mut setup_sql = match &dump.schema {
        Some(snapshot) if !snapshot.tables.is_empty() => {
            let diffs: Vec<SchemaDiff> = snapshot
                .to_schemas()?
                .into_iter()
                .map(SchemaDiff::CreateTable)
                .collect();
            Some(generate_sql_from_diff(&diffs, backend))
        }
        _ => None,
    };

    let mut rows = Vec::new();
    // Mark the restored migrations as applied, in the same transaction as the schema, so
    // `rango migrate` does not replay them. Only when the schema is created here: a data-only
    // restore into a migrated database must not touch the tracking table.
    if let Some(sql) = &mut setup_sql {
        match &dump.migrations {
            Some(names) => {
                sql.push('\n');
                sql.push_str(tracking_table_ddl(backend));
                sql.push(';');
                for name in names {
                    // Bound at insert time like every other value.
                    rows.push(serde_json::Map::from_iter([
                        ("__table".to_string(), MIGRATIONS_TABLE.into()),
                        ("name".to_string(), name.clone().into()),
                    ]));
                }
            }
            None => eprintln!(
                "warning: this dump does not list applied migrations (made by an older rango), \
                 so none were recorded. Check `rango migrate` before running it: it may replay \
                 migrations already contained in the restored schema."
            ),
        }
    }
    for table in dump.tables {
        for mut row in table.rows {
            row.insert(
                "__table".to_string(),
                serde_json::Value::String(table.name.clone()),
            );
            rows.push(row);
        }
    }

    let count = import::insert_rows(
        backend,
        database_url,
        &rows,
        None,
        import::OnConflict::Fail,
        setup_sql.as_deref(),
    )
    .await?;
    println!("✅ Restored {} row(s) from {}", count, input);
    Ok(())
}

fn parse_dump(bytes: &[u8]) -> Result<Dump> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).context("Not a Rango dump: invalid JSON")?;
    if value.get("format").and_then(|f| f.as_str()) != Some(FORMAT) {
        bail!("Not a Rango dump: expected \"format\": \"{}\"", FORMAT);
    }
    match value.get("version").and_then(|v| v.as_u64()) {
        Some(1 | 2) => {}
        other => bail!(
            "Unsupported dump version {} (this rango reads versions 1 and {})",
            other.map_or("(missing)".to_string(), |v| v.to_string()),
            VERSION
        ),
    }
    serde_json::from_value(value).context("Malformed Rango dump")
}

fn validate(dump: &Dump) -> Result<()> {
    let mut seen = HashSet::new();
    for table in &dump.tables {
        if table.name.is_empty() || !seen.insert(table.name.as_str()) {
            bail!("Invalid or duplicate table name '{}'", table.name);
        }
        // With a schema, every table and column must be declared in it. Without one,
        // the target database is the judge: unknown names fail the insert.
        let columns: Option<HashSet<&str>> = match &dump.schema {
            Some(schema) => {
                let Some(t) = schema.tables.get(&table.name) else {
                    bail!("Table '{}' is not in the dump schema", table.name);
                };
                Some(t.columns.iter().map(|c| c.name.as_str()).collect())
            }
            None => None,
        };
        let mut keys: Option<BTreeSet<&str>> = None;
        for row in &table.rows {
            let row_keys: BTreeSet<&str> = row.keys().map(String::as_str).collect();
            if let Some(cols) = &columns
                && let Some(bad) = row_keys.iter().find(|k| !cols.contains(*k))
            {
                bail!(
                    "Column '{}' is not in the schema of table '{}'",
                    bad,
                    table.name
                );
            }
            if keys.get_or_insert_with(|| row_keys.clone()) != &row_keys {
                bail!(
                    "Rows of table '{}' do not all have the same columns",
                    table.name
                );
            }
        }
    }
    Ok(())
}

/// Write `bytes` to `path`, owner-only (0600) from creation on unix.
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    opts.open(path)
        .and_then(|mut f| f.write_all(bytes))
        .with_context(|| format!("Failed to write {}", path.display()))
}

/// Current UTC time as (ISO 8601, `YYYYMMDD-HHMMSS`).
fn utc_now() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    civil(secs)
}

fn civil(secs: i64) -> (String, String) {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let (h, mi, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    // Days since 1970-01-01 to a civil date (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z"),
        format!("{y:04}{m:02}{d:02}-{h:02}{mi:02}{s:02}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_formats_known_instants() {
        assert_eq!(civil(0).0, "1970-01-01T00:00:00Z");
        assert_eq!(civil(1_793_966_400).1, "20261106-120000");
    }
}
