use anyhow::Result;
use clap::{Parser, Subcommand};

mod config;
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
        Command::Makemigrations { path, output, prefix, dry_run, check } => {
            let cfg = config::RangoConfig::load()?;
            let prefix = prefix
                .or(cfg.models.prefix)
                .map(|s| s.as_str().to_string());
            makemigrations::run(&path, &output, prefix.as_deref(), dry_run, check)?;
        }
        Command::Migrate { database_url, migrations } => {
            let cfg = config::RangoConfig::load()?;
            let url = cfg.resolve_database_url(database_url.as_deref())?;
            let dir = if migrations == "migrations" {
                cfg.migrations.dir.clone()
            } else {
                migrations
            };
            tokio::runtime::Runtime::new()?
                .block_on(migrate::run(&url, &dir, &cfg.database.after_connect))?;
        }
    }

    Ok(())
}
