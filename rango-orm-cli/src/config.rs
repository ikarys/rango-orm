use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;

/// Configuration loaded from rango.toml
#[derive(Deserialize, Default)]
pub struct RangoConfig {
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub migrations: MigrationsConfig,
    #[serde(default)]
    pub models: ModelsConfig,
}

#[allow(dead_code)]
#[derive(Deserialize)]
pub struct DatabaseConfig {
    /// Database backend: "sqlite", "postgres" or "mysql" (alias "mariadb"); default "postgres".
    #[serde(default = "default_backend")]
    pub backend: String,

    /// Database URL — read from rango.toml for CLI tools (makemigrations, migrate).
    /// In production, prefer DATABASE_URL env var to avoid committing credentials.
    #[serde(default)]
    pub url: Option<String>,
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
            backend: default_backend(),
            url: None,
            max_connections: default_max_connections(),
            min_connections: default_min_connections(),
            connect_timeout: default_connect_timeout(),
            idle_timeout: default_idle_timeout(),
            after_connect: Vec::new(),
        }
    }
}

impl DatabaseConfig {
    pub fn backend_kind(&self) -> rango_orm_core::BackendKind {
        match self.backend.as_str() {
            "sqlite" => rango_orm_core::BackendKind::Sqlite,
            "mysql" | "mariadb" => rango_orm_core::BackendKind::Mysql,
            _ => rango_orm_core::BackendKind::Postgres,
        }
    }
}

/// Backend named by a database URL scheme, if it is one Rango knows.
pub fn backend_from_url(url: &str) -> Option<rango_orm_core::BackendKind> {
    use rango_orm_core::BackendKind::*;
    match url.split_once(':')?.0.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" => Some(Postgres),
        "mysql" | "mariadb" => Some(Mysql),
        "sqlite" => Some(Sqlite),
        _ => None,
    }
}

fn default_max_connections() -> u32 {
    20
}
fn default_min_connections() -> u32 {
    2
}
fn default_connect_timeout() -> u64 {
    10
}
fn default_idle_timeout() -> u64 {
    600
}

#[allow(dead_code)]
#[derive(Deserialize)]
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

#[allow(dead_code)]
#[derive(Deserialize)]
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

fn default_backend() -> String {
    "postgres".to_string()
}
fn default_migrations_dir() -> String {
    "migrations".to_string()
}
fn default_migrations_table() -> String {
    "_rango_migrations".to_string()
}
fn default_src_dir() -> String {
    "src".to_string()
}

impl RangoConfig {
    /// Backend for a command that talks to `url`: the URL scheme wins over `rango.toml`.
    pub fn backend_for(&self, url: &str) -> rango_orm_core::BackendKind {
        backend_from_url(url).unwrap_or_else(|| self.database.backend_kind())
    }

    /// Load from rango.toml if it exists, otherwise return defaults.
    pub fn load() -> Result<Self> {
        let path = "rango.toml";
        if !std::path::Path::new(path).exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path).context("Failed to read rango.toml")?;
        let config: Self = toml::from_str(&content).context("Failed to parse rango.toml")?;
        Ok(config)
    }

    /// Resolve the database URL with priority:
    /// 1. CLI argument --database-url
    /// 2. DATABASE_URL env var
    /// 3. `url` in rango.toml (local dev only)
    pub fn resolve_database_url(&self, cli_url: Option<&str>) -> Result<String> {
        if let Some(url) = cli_url {
            return Ok(url.to_string());
        }
        if let Ok(url) = std::env::var("DATABASE_URL") {
            return Ok(url);
        }
        if let Some(url) = &self.database.url {
            return Ok(url.clone());
        }
        anyhow::bail!(
            "No database URL found.\n\
             Set DATABASE_URL env var or use --database-url.\n\
             (`url` in rango.toml also works for local dev; never store credentials there.)"
        )
    }

    /// URL for `rango migrate`: CLI argument, then `MIGRATE_DATABASE_URL`, then `DATABASE_URL`.
    /// Lets migrations run with a DDL role while the app's `DATABASE_URL` is DML-only.
    pub fn resolve_migrate_url(&self, cli_url: Option<&str>) -> Result<String> {
        if cli_url.is_none()
            && let Ok(url) = std::env::var("MIGRATE_DATABASE_URL")
        {
            return Ok(url);
        }
        self.resolve_database_url(cli_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rango_orm_core::BackendKind::*;

    #[test]
    fn toml_url_is_the_last_fallback() {
        let cfg = RangoConfig {
            database: DatabaseConfig {
                url: Some("sqlite://dev.db".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            cfg.resolve_database_url(Some("sqlite://cli.db")).unwrap(),
            "sqlite://cli.db"
        );
        if std::env::var("DATABASE_URL").is_err() {
            assert_eq!(cfg.resolve_database_url(None).unwrap(), "sqlite://dev.db");
        }
    }

    #[test]
    fn url_scheme_overrides_configured_backend() {
        let cfg = RangoConfig {
            database: DatabaseConfig {
                backend: "sqlite".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(cfg.backend_for("postgres://u@h/db"), Postgres);
        assert_eq!(cfg.backend_for("postgresql://u@h/db"), Postgres);
        assert_eq!(cfg.backend_for("mariadb://u@h/db"), Mysql);
        assert_eq!(cfg.backend_for("MySQL://u@h/db"), Mysql);
        assert_eq!(cfg.backend_for("sqlite://a.db"), Sqlite);
        assert_eq!(
            cfg.backend_for("a.db"),
            Sqlite,
            "unknown scheme: rango.toml"
        );
    }
}
