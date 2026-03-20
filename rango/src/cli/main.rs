use anyhow::Result;
use clap::{Parser, Subcommand};

mod config;
mod makemigrations;
mod migrate;
mod scanner;


#[derive(Parser)]
#[command(name = "rango", about = "Rango ORM CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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
        Command::Makemigrations { path, output, prefix } => {
            let cfg = config::RangoConfig::load()?;
            let prefix = prefix
                .or(cfg.models.prefix)
                .map(|s| s.as_str().to_string());
            makemigrations::run(&path, &output, prefix.as_deref())?;
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
                .block_on(migrate::run(&url, &dir))?;
        }
    }

    Ok(())
}
