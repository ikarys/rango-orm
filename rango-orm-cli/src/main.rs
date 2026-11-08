#![forbid(unsafe_code)]
use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod config;
mod migration;

use commands::{dump, export, import, init, makemigrations, migrate};

#[derive(Parser)]
#[command(name = "rango", about = "Rango ORM CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize a new Rango project (creates rango.toml and migrations/)
    Init {
        /// Database backend: sqlite (default), postgres or mysql (alias mariadb)
        #[arg(long, default_value = "sqlite")]
        backend: String,
    },

    /// Scan models and generate migration files
    Makemigrations {
        /// Directory to scan (default: `[models] src` in rango.toml, else src/)
        path: Option<String>,

        /// Output directory for migration files (default: migrations/)
        #[arg(short, long, default_value = "migrations")]
        output: String,

        /// Print the SQL that would be generated without writing any files
        #[arg(long)]
        dry_run: bool,

        /// Exit with code 1 if there are pending migrations (useful in CI)
        #[arg(long)]
        check: bool,

        /// Answer yes to every prompt (renames, destructive changes). Review the SQL!
        #[arg(short, long)]
        yes: bool,
    },

    /// Export data to JSON or CSV
    Export {
        /// Table to export (default: all tables)
        #[arg(short, long)]
        table: Option<String>,

        /// Output format: json (default) or csv
        #[arg(short, long, default_value = "json")]
        format: String,

        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<String>,

        /// Database URL (overrides DATABASE_URL env var)
        #[arg(short, long)]
        database_url: Option<String>,
    },

    /// Import data from JSON or CSV
    Import {
        /// Input file path
        input: String,

        /// Table to import into (required for CSV without __table column)
        #[arg(short, long)]
        table: Option<String>,

        /// Format: json or csv (auto-detected from extension)
        #[arg(short, long)]
        format: Option<String>,

        /// Replace existing rows on conflict (default: skip)
        #[arg(long)]
        replace: bool,

        /// Database URL (overrides DATABASE_URL env var)
        #[arg(short, long)]
        database_url: Option<String>,
    },

    /// Dump schema and data to a backend-independent Rango JSON file
    Dump {
        /// Dump the schema snapshot only (no rows)
        #[arg(long, conflicts_with = "data_only")]
        schema_only: bool,

        /// Dump the rows only (no schema)
        #[arg(long)]
        data_only: bool,

        /// Table to dump; repeat for several (default: all tables)
        #[arg(short, long)]
        table: Vec<String>,

        /// Output file, written with 0600 permissions (default: stdout)
        #[arg(short, long)]
        output: Option<String>,

        /// Directory containing the schema snapshot (default: migrations/)
        #[arg(short, long, default_value = "migrations")]
        migrations: String,

        /// Database URL (overrides DATABASE_URL env var)
        #[arg(short, long)]
        database_url: Option<String>,
    },

    /// Restore a Rango dump (plain JSON or gzip) in one transaction
    Restore {
        /// Dump file
        input: String,

        /// Database URL (overrides DATABASE_URL env var)
        #[arg(short, long)]
        database_url: Option<String>,
    },

    /// Write a gzip-compressed, timestamped full dump
    Backup {
        /// Directory for backup files (created if missing)
        #[arg(long, default_value = "backups")]
        dir: String,

        /// Database URL (overrides DATABASE_URL env var)
        #[arg(short, long)]
        database_url: Option<String>,
    },

    /// Apply pending migrations to the database
    Migrate {
        /// Database URL (overrides MIGRATE_DATABASE_URL, then DATABASE_URL)
        #[arg(short, long)]
        database_url: Option<String>,

        /// Directory containing migration files (default: migrations/)
        #[arg(short, long, default_value = "migrations")]
        migrations: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init { backend } => {
            init::run(&backend)?;
        }
        Command::Makemigrations {
            path,
            output,
            dry_run,
            check,
            yes,
        } => {
            let cfg = config::RangoConfig::load()?;
            if cfg.models.prefix.is_some() {
                anyhow::bail!(
                    "`[models] prefix` in rango.toml is no longer supported: table prefixes were removed \
                     because the runtime never applied them. Remove it and use `#[model(table = \"...\")]` instead"
                );
            }
            let path = path.unwrap_or_else(|| cfg.models.src.clone());
            makemigrations::run(&path, &output, dry_run, check, yes)?;
        }
        Command::Export {
            table,
            format,
            output,
            database_url,
        } => {
            let cfg = config::RangoConfig::load()?;
            let url = cfg.resolve_database_url(database_url.as_deref())?;
            let fmt = export::Format::from_str(&format)?;
            tokio::runtime::Runtime::new()?.block_on(export::run(
                &url,
                table.as_deref(),
                fmt,
                output.as_deref(),
            ))?;
        }

        Command::Import {
            input,
            table,
            format,
            replace,
            database_url,
        } => {
            let cfg = config::RangoConfig::load()?;
            let url = cfg.resolve_database_url(database_url.as_deref())?;
            tokio::runtime::Runtime::new()?.block_on(import::run(
                &url,
                &input,
                table.as_deref(),
                format.as_deref(),
                replace,
            ))?;
        }

        Command::Dump {
            schema_only,
            data_only,
            table,
            output,
            migrations,
            database_url,
        } => {
            let cfg = config::RangoConfig::load()?;
            let url = cfg.resolve_database_url(database_url.as_deref())?;
            let dir = if migrations == "migrations" {
                cfg.migrations.dir.clone()
            } else {
                migrations
            };
            tokio::runtime::Runtime::new()?.block_on(dump::dump(
                &url,
                &table,
                schema_only,
                data_only,
                &dir,
                output.as_deref(),
            ))?;
        }

        Command::Restore {
            input,
            database_url,
        } => {
            let cfg = config::RangoConfig::load()?;
            let url = cfg.resolve_database_url(database_url.as_deref())?;
            tokio::runtime::Runtime::new()?.block_on(dump::restore(&url, &input))?;
        }

        Command::Backup { dir, database_url } => {
            let cfg = config::RangoConfig::load()?;
            let url = cfg.resolve_database_url(database_url.as_deref())?;
            tokio::runtime::Runtime::new()?.block_on(dump::backup(
                &url,
                &dir,
                &cfg.migrations.dir,
            ))?;
        }

        Command::Migrate {
            database_url,
            migrations,
        } => {
            let cfg = config::RangoConfig::load()?;
            let url = cfg.resolve_migrate_url(database_url.as_deref())?;
            let dir = if migrations == "migrations" {
                cfg.migrations.dir.clone()
            } else {
                migrations
            };
            tokio::runtime::Runtime::new()?.block_on(migrate::run(
                &url,
                &dir,
                &cfg.database.after_connect,
            ))?;
        }
    }

    Ok(())
}
