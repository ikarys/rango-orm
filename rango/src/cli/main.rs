use anyhow::Result;
use clap::{Parser, Subcommand};

mod config;
mod export;
mod import;
mod init;
mod makemigrations;
mod migrate;
mod scanner;
mod snapshot;

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
        /// Database backend: sqlite (default) or postgres
        #[arg(long, default_value = "sqlite")]
        backend: String,
    },

    /// Scan models and generate migration files
    Makemigrations {
        /// Directory to scan (default: src/)
        #[arg(default_value = "src")]
        path: String,

        /// Output directory for migration files (default: migrations/)
        #[arg(short, long, default_value = "migrations")]
        output: String,

        /// Table prefix (default: auto-detected from Cargo.toml)
        #[arg(short, long)]
        prefix: Option<String>,

        /// Print the SQL that would be generated without writing any files
        #[arg(long)]
        dry_run: bool,

        /// Exit with code 1 if there are pending migrations (useful in CI)
        #[arg(long)]
        check: bool,
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

    /// Apply pending migrations to the database
    Migrate {
        /// Database URL (overrides DATABASE_URL from .env)
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
            prefix,
            dry_run,
            check,
        } => {
            let cfg = config::RangoConfig::load()?;
            let prefix = prefix.or(cfg.models.prefix).map(|s| s.as_str().to_string());
            makemigrations::run(&path, &output, prefix.as_deref(), dry_run, check)?;
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

        Command::Migrate {
            database_url,
            migrations,
        } => {
            let cfg = config::RangoConfig::load()?;
            let url = cfg.resolve_database_url(database_url.as_deref())?;
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
