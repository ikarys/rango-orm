use anyhow::{Context, Result, bail};
use rango_orm_core::BackendKind;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

/// Fixed key for the session-level advisory lock used by `rango migrate`.
const MIGRATION_LOCK_KEY: i64 = 0x72616e676f;

pub async fn run(database_url: &str, migrations_dir: &str, after_connect: &[String]) -> Result<()> {
    // Detect backend from rango.toml
    let cfg = crate::config::RangoConfig::load().unwrap_or_default();
    let backend = cfg.database.backend_kind();

    println!(
        "🔌 Connecting to database ({})...",
        match backend {
            BackendKind::Sqlite => "sqlite",
            BackendKind::Mysql => "mysql",
            _ => "postgres",
        }
    );

    match backend {
        BackendKind::Sqlite => run_sqlite(database_url, migrations_dir).await,
        BackendKind::Mysql => run_mysql(database_url, migrations_dir).await,
        _ => run_postgres(database_url, migrations_dir, after_connect).await,
    }
}

// ─── Postgres ─────────────────────────────────────────────────────────────────

async fn run_postgres(
    database_url: &str,
    migrations_dir: &str,
    after_connect: &[String],
) -> Result<()> {
    let mut config = rango_orm_core::DatabaseConfig::from_url(database_url);
    config.after_connect = after_connect.to_vec();
    let pool = rango_orm_postgres::connect(&config)
        .await
        .context("Failed to connect to Postgres")?;

    let mut conn = pool
        .acquire()
        .await
        .context("Failed to acquire a Postgres connection")?;

    // Take a session-level advisory lock so concurrent `rango migrate`
    // processes cannot apply the same migration at the same time.
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(MIGRATION_LOCK_KEY)
        .execute(&mut *conn)
        .await
        .context("Failed to acquire the migration advisory lock")?;

    let result = async {
        sqlx::query(
            tracking_table_ddl(BackendKind::Postgres),
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create _rango_migrations")?;

        apply_pending::<sqlx::Postgres>(
            &mut *conn,
            migrations_dir,
            "INSERT INTO _rango_migrations (name) VALUES ($1)",
            None,
        )
        .await
    }
    .await;

    let unlock = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(MIGRATION_LOCK_KEY)
        .execute(&mut *conn)
        .await;

    finish(
        result,
        unlock,
        "Failed to release the migration advisory lock",
    )
}

// ─── SQLite ───────────────────────────────────────────────────────────────────

async fn run_sqlite(database_url: &str, migrations_dir: &str) -> Result<()> {
    let pool = rango_orm_sqlite::connect(database_url)
        .await
        .context("Failed to connect to SQLite")?;

    // SQLite serialises writers, so no advisory lock is needed.
    sqlx::query(
        tracking_table_ddl(BackendKind::Sqlite),
    )
    .execute(&pool)
    .await
    .context("Failed to create _rango_migrations")?;

    let mut conn = pool
        .acquire()
        .await
        .context("Failed to acquire a SQLite connection")?;

    // `PRAGMA foreign_keys` is a no-op inside a transaction, and a rebuild's DROP TABLE
    // would otherwise fire ON DELETE CASCADE on child tables. Disable it around the
    // migration, then re-enable it afterwards.
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *conn)
        .await
        .context("Failed to disable foreign key checks")?;

    let result = apply_pending::<sqlx::Sqlite>(
        &mut *conn,
        migrations_dir,
        "INSERT INTO _rango_migrations (name) VALUES (?)",
        Some("PRAGMA foreign_key_check"),
    )
    .await;

    let enable = sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *conn)
        .await;

    finish(result, enable, "Failed to re-enable foreign key checks")
}

// ─── MySQL ────────────────────────────────────────────────────────────────────

/// Run migrations against MySQL/MariaDB.
///
/// MySQL DDL auto-commits, so the per-migration transaction only makes the bookkeeping insert atomic.
async fn run_mysql(database_url: &str, migrations_dir: &str) -> Result<()> {
    let pool = rango_orm_mysql::connect(database_url)
        .await
        .context("Failed to connect to MySQL")?;

    let mut conn = pool
        .acquire()
        .await
        .context("Failed to acquire a MySQL connection")?;

    let lock = sqlx::query_scalar::<_, Option<i64>>("SELECT GET_LOCK('rango_migrate', 3600)")
        .fetch_one(&mut *conn)
        .await
        .context("Failed to acquire the migration lock")?;

    if lock != Some(1) {
        bail!("Failed to acquire the migration lock");
    }

    let result = async {
        sqlx::query(
            tracking_table_ddl(BackendKind::Mysql),
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create _rango_migrations")?;

        apply_pending::<sqlx::MySql>(
            &mut *conn,
            migrations_dir,
            "INSERT INTO _rango_migrations (name) VALUES (?)",
            None,
        )
        .await
    }
    .await;

    let unlock = sqlx::query("SELECT RELEASE_LOCK('rango_migrate')")
        .execute(&mut *conn)
        .await;

    finish(result, unlock, "Failed to release the migration lock")
}

/// `CREATE TABLE IF NOT EXISTS` for the migration tracking table (also used by `restore`).
pub(crate) fn tracking_table_ddl(backend: BackendKind) -> &'static str {
    match backend {
        BackendKind::Sqlite => "CREATE TABLE IF NOT EXISTS _rango_migrations (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT NOT NULL UNIQUE,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        BackendKind::Mysql => "CREATE TABLE IF NOT EXISTS _rango_migrations (
                id         BIGINT AUTO_INCREMENT PRIMARY KEY,
                name       VARCHAR(255) NOT NULL UNIQUE,
                applied_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        _ => "CREATE TABLE IF NOT EXISTS _rango_migrations (
                id         SERIAL PRIMARY KEY,
                name       TEXT NOT NULL UNIQUE,
                applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
            )",
    }
}

// ─── Shared logic ─────────────────────────────────────────────────────────────

/// Result of the migrations, or the cleanup error (unlock, re-enable FKs) when they succeeded:
/// a migration error is never masked by a failed cleanup.
fn finish<T>(
    result: Result<()>,
    cleanup: std::result::Result<T, sqlx::Error>,
    what: &str,
) -> Result<()> {
    match cleanup {
        Err(err) if result.is_ok() => Err(anyhow::Error::from(err).context(what.to_string())),
        _ => result,
    }
}

/// Apply every pending migration, each in its own transaction. Backend-agnostic.
async fn apply_pending<DB: sqlx::Database>(
    conn: &mut DB::Connection,
    migrations_dir: &str,
    record_sql: &str,
    fk_check: Option<&str>,
) -> Result<()>
where
    for<'c> &'c mut DB::Connection: sqlx::Executor<'c, Database = DB>,
    for<'q> String: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    usize: sqlx::ColumnIndex<DB::Row>,
    DB::Arguments: sqlx::IntoArguments<DB>,
{
    let rows: Vec<String> = sqlx::query_scalar("SELECT name FROM _rango_migrations")
        .fetch_all(&mut *conn)
        .await
        .context("Failed to fetch applied migrations")?;
    let applied: HashSet<String> = rows.into_iter().collect();

    let pending = pending_migrations(migrations_dir, &applied)?;
    if pending.is_empty() {
        println!("✅ Nothing to apply — database is up to date.");
        return Ok(());
    }

    let count = pending.len();
    for (name, sql) in pending {
        println!("  ▶  Applying {}...", name);
        let mut tx = sqlx::Connection::begin(&mut *conn)
            .await
            .with_context(|| format!("Failed to begin transaction for {}", name))?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(&*sql))
            .execute(&mut *tx)
            .await
            .with_context(|| format!("Failed to apply {}", name))?;
        if let Some(fk_check) = fk_check
            && !sqlx::raw_sql(sqlx::AssertSqlSafe(fk_check))
                .fetch_all(&mut *tx)
                .await
                .with_context(|| format!("Failed to check foreign keys for {}", name))?
                .is_empty()
        {
            bail!("{name} leaves foreign key violations; rolled back");
        }
        sqlx::query(sqlx::AssertSqlSafe(record_sql))
            .bind(name.clone())
            .execute(&mut *tx)
            .await
            .with_context(|| format!("Failed to record migration {}", name))?;
        tx.commit()
            .await
            .with_context(|| format!("Failed to commit migration {}", name))?;
        println!("  ✅ {}", name);
    }
    println!("✅ Applied {} migration(s).", count);
    Ok(())
}

fn pending_migrations(
    migrations_dir: &str,
    applied: &HashSet<String>,
) -> Result<Vec<(String, String)>> {
    let files = collect_migration_files(migrations_dir)?;
    let mut pending = Vec::new();

    for file in files {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        if applied.contains(&name) {
            continue;
        }

        let sql = fs::read_to_string(&file)
            .with_context(|| format!("Failed to read {}", file.display()))?;
        pending.push((name, sql));
    }

    Ok(pending)
}

fn collect_migration_files(dir: &str) -> Result<Vec<PathBuf>> {
    let entries =
        fs::read_dir(dir).with_context(|| format!("Cannot read migrations directory: {}", dir))?;
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "sql").unwrap_or(false))
        .collect();
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_migrations_returns_only_unapplied_sql_files() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rango-migrate-test-{}-{}",
            std::process::id(),
            unique
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(dir.join("0001_a.sql"), "SELECT 1;").unwrap();
        fs::write(dir.join("0002_b.sql"), "SELECT 2;").unwrap();
        fs::write(dir.join("notes.txt"), "not a migration").unwrap();

        let applied = HashSet::from(["0001_a.sql".to_string()]);
        let pending = pending_migrations(dir.to_str().unwrap(), &applied).unwrap();

        assert_eq!(
            pending,
            vec![("0002_b.sql".to_string(), "SELECT 2;".to_string())]
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
