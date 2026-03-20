use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;

/// Configuration loaded from rango.toml
#[derive(Debug, Deserialize, Default)]
pub struct RangoConfig {
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub migrations: MigrationsConfig,
    #[serde(default)]
    pub models: ModelsConfig,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout: u64,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout: u64,
    /// SQL statements executed on every new connection.
    /// Useful for: strict mode, timezone, search_path, charset, etc.
    /// Example: ["SET time_zone = '+00:00'", "SET SESSION sql_mode = 'STRICT_TRANS_TABLES'"]
    #[serde(default)]
    pub after_connect: Vec<String>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            max_connections: default_max_connections(),
            min_connections: default_min_connections(),
            connect_timeout: default_connect_timeout(),
            idle_timeout: default_idle_timeout(),
            after_connect: Vec::new(),
        }
    }
}

fn default_max_connections() -> u32 { 20 }
fn default_min_connections() -> u32 { 2 }
fn default_connect_timeout() -> u64 { 10 }
fn default_idle_timeout() -> u64 { 600 }

#[derive(Debug, Deserialize)]
pub struct MigrationsConfig {
    #[serde(default = "default_migrations_dir")]
    pub dir: String,
    #[serde(default = "default_migrations_table")]
    pub table: String,
}

impl Default for MigrationsConfig {
    fn default() -> Self {
        Self {
            dir: default_migrations_dir(),
            table: default_migrations_table(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ModelsConfig {
    pub prefix: Option<String>,
    #[serde(default = "default_src_dir")]
    pub src: String,
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            prefix: None,
            src: default_src_dir(),
        }
    }
}

fn default_migrations_dir() -> String { "migrations".to_string() }
fn default_migrations_table() -> String { "_rango_migrations".to_string() }
fn default_src_dir() -> String { "src".to_string() }

impl RangoConfig {
    /// Load from rango.toml if it exists, otherwise return defaults.
    pub fn load() -> Result<Self> {
        let path = "rango.toml";
        if !std::path::Path::new(path).exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)
            .context("Failed to read rango.toml")?;
        let config: Self = toml::from_str(&content)
            .context("Failed to parse rango.toml")?;
        Ok(config)
    }

    /// Resolve the database URL with priority:
    /// 1. CLI argument --database-url
    /// 2. DATABASE_URL env var
    pub fn resolve_database_url(&self, cli_url: Option<&str>) -> Result<String> {
        if let Some(url) = cli_url {
            return Ok(url.to_string());
        }
        if let Ok(url) = std::env::var("DATABASE_URL") {
            return Ok(url);
        }
        anyhow::bail!(
            "No database URL found.\n\
             Set DATABASE_URL env var or use --database-url.\n\
             Note: database URL should not be stored in rango.toml (use env vars for secrets)."
        )
    }
}
