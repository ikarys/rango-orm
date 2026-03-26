use anyhow::{Context, Result};
use rango_core::BackendKind;
use std::fs;

pub async fn run(database_url: &str, migrations_dir: &str, after_connect: &[String]) -> Result<()> {
    // Detect backend from rango.toml
    let cfg = crate::config::RangoConfig::load().unwrap_or_default();
    let backend = cfg.database.backend_kind();

    println!("🔌 Connecting to database ({})...",
        match backend { BackendKind::Sqlite => "sqlite", _ => "postgres" });

    match backend {
        BackendKind::Sqlite => run_sqlite(database_url, migrations_dir).await,
        _                   => run_postgres(database_url, migrations_dir, after_connect).await,
    }
}

// ─── Postgres ─────────────────────────────────────────────────────────────────

async fn run_postgres(database_url: &str, migrations_dir: &str, after_connect: &[String]) -> Result<()> {
    let mut config = rango_core::DatabaseConfig::from_url(database_url);
    config.after_connect = after_connect.to_vec();
    let pool = rango_postgres::connect(&config).await
        .context("Failed to connect to Postgres")?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _rango_migrations (
            id         SERIAL PRIMARY KEY,
            name       TEXT NOT NULL UNIQUE,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"
    ).execute(&pool).await.context("Failed to create _rango_migrations")?;

    let applied = fetch_applied_pg(&pool).await?;
    apply_files(migrations_dir, &applied, |sql, name| {
        let pool = pool.clone();
        let sql = sql.to_string();
        let name = name.to_string();
        async move {
            sqlx::raw_sql(&sql).execute(&pool).await
                .with_context(|| format!("Failed to apply {}", name))?;
            sqlx::query("INSERT INTO _rango_migrations (name) VALUES ($1)")
                .bind(&name).execute(&pool).await
                .context("Failed to record migration")?;
            Ok(())
        }
    }).await
}

async fn fetch_applied_pg(pool: &rango_postgres::PgPool) -> Result<std::collections::HashSet<String>> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT name FROM _rango_migrations")
        .fetch_all(pool).await.context("Failed to fetch applied migrations")?;
    Ok(rows.into_iter().map(|(n,)| n).collect())
}

// ─── SQLite ───────────────────────────────────────────────────────────────────

async fn run_sqlite(database_url: &str, migrations_dir: &str) -> Result<()> {
    let pool = rango_sqlite::connect(database_url).await
        .context("Failed to connect to SQLite")?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _rango_migrations (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT NOT NULL UNIQUE,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )"
    ).execute(&pool).await.context("Failed to create _rango_migrations")?;

    let applied = fetch_applied_sqlite(&pool).await?;
    apply_files(migrations_dir, &applied, |sql, name| {
        let pool = pool.clone();
        let sql = sql.to_string();
        let name = name.to_string();
        async move {
            sqlx::raw_sql(&sql).execute(&pool).await
                .with_context(|| format!("Failed to apply {}", name))?;
            sqlx::query("INSERT INTO _rango_migrations (name) VALUES (?)")
                .bind(&name).execute(&pool).await
                .context("Failed to record migration")?;
            Ok(())
        }
    }).await
}

async fn fetch_applied_sqlite(pool: &rango_sqlite::SqlitePool) -> Result<std::collections::HashSet<String>> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT name FROM _rango_migrations")
        .fetch_all(pool).await.context("Failed to fetch applied migrations")?;
    Ok(rows.into_iter().map(|(n,)| n).collect())
}

// ─── Shared logic ─────────────────────────────────────────────────────────────

async fn apply_files<F, Fut>(
    migrations_dir: &str,
    applied: &std::collections::HashSet<String>,
    apply_fn: F,
) -> Result<()>
where
    F: Fn(&str, &str) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let mut files = collect_migration_files(migrations_dir)?;
    files.sort();

    if files.is_empty() {
        println!("No migration files found in {}", migrations_dir);
        return Ok(());
    }

    let mut count = 0;
    for file in &files {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        if applied.contains(&name) {
            println!("  ⏭  {} (already applied)", name);
            continue;
        }
        println!("  ▶  Applying {}...", name);
        let sql = fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;
        apply_fn(&sql, &name).await?;
        println!("  ✅ {}", name);
        count += 1;
    }

    if count == 0 {
        println!("✅ Nothing to apply — database is up to date.");
    } else {
        println!("✅ Applied {} migration(s).", count);
    }
    Ok(())
}

fn collect_migration_files(dir: &str) -> Result<Vec<std::path::PathBuf>> {
    let entries = fs::read_dir(dir)
        .with_context(|| format!("Cannot read migrations directory: {}", dir))?;
    Ok(entries.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "sql").unwrap_or(false))
        .collect())
}
