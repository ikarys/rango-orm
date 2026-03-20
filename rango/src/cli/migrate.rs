use anyhow::{Context, Result};
use sqlx::PgPool;
use std::fs;

pub async fn run(database_url: &str, migrations_dir: &str) -> Result<()> {
    println!("🔌 Connecting to database...");
    let pool = PgPool::connect(database_url).await
        .context("Failed to connect to database")?;

    ensure_migrations_table(&pool).await?;

    let mut files = collect_migration_files(migrations_dir)?;
    files.sort();

    if files.is_empty() {
        println!("No migration files found in {}", migrations_dir);
        return Ok(());
    }

    let applied = get_applied_migrations(&pool).await?;
    let mut count = 0;

    for file in &files {
        let name = file.file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        if applied.contains(&name) {
            println!("  ⏭  {} (already applied)", name);
            continue;
        }

        println!("  ▶  Applying {}...", name);
        let sql = fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;

        sqlx::raw_sql(&sql)
            .execute(&pool)
            .await
            .with_context(|| format!("Failed to apply {}", name))?;

        record_migration(&pool, &name).await?;
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

async fn ensure_migrations_table(pool: &PgPool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _rango_migrations (
            id          SERIAL PRIMARY KEY,
            name        TEXT NOT NULL UNIQUE,
            applied_at  TIMESTAMPTZ NOT NULL DEFAULT now()
        )"
    )
    .execute(pool)
    .await
    .context("Failed to create _rango_migrations table")?;
    Ok(())
}

async fn get_applied_migrations(pool: &PgPool) -> Result<std::collections::HashSet<String>> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT name FROM _rango_migrations")
        .fetch_all(pool)
        .await
        .context("Failed to fetch applied migrations")?;
    Ok(rows.into_iter().map(|(name,)| name).collect())
}

async fn record_migration(pool: &PgPool, name: &str) -> Result<()> {
    sqlx::query("INSERT INTO _rango_migrations (name) VALUES ($1)")
        .bind(name)
        .execute(pool)
        .await
        .context("Failed to record migration")?;
    Ok(())
}

fn collect_migration_files(dir: &str) -> Result<Vec<std::path::PathBuf>> {
    let entries = fs::read_dir(dir)
        .with_context(|| format!("Cannot read migrations directory: {}", dir))?;

    let files = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "sql").unwrap_or(false))
        .collect();

    Ok(files)
}
