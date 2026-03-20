use anyhow::Result;
use clap::{Parser, Subcommand};

mod scanner;
mod makemigrations;


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
    },

    /// Apply pending migrations to the database
    Migrate {
        /// Database URL (or set DATABASE_URL env var)
        #[arg(short, long, env = "DATABASE_URL")]
        database_url: String,

        /// Directory containing migration files (default: migrations/)
        #[arg(short, long, default_value = "migrations")]
        migrations: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Makemigrations { path, output } => {
            makemigrations::run(&path, &output)?;
        }
        Command::Migrate { database_url, migrations } => {
            println!("migrate: not yet implemented (db={database_url}, dir={migrations})");
        }
    }

    Ok(())
}
