/// Database connection configuration.
/// Backend-agnostic — works for PostgreSQL, MySQL, MariaDB, SQLite.
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
}

impl DatabaseConfig {
    /// Build from a full connection URL.
    /// Examples:
    ///   postgres://user:pass@localhost:5432/mydb
    ///   mysql://user:pass@localhost:3306/mydb
    ///   sqlite:///path/to/db.sqlite3
    pub fn from_url(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }

    /// Build from environment variable (defaults to DATABASE_URL).
    pub fn from_env() -> Result<Self, std::env::VarError> {
        Self::from_env_var("DATABASE_URL")
    }

    pub fn from_env_var(var: &str) -> Result<Self, std::env::VarError> {
        let url = std::env::var(var)?;
        Ok(Self::from_url(url))
    }

    /// Detect the backend kind from the URL scheme.
    pub fn backend(&self) -> BackendKind {
        if self.url.starts_with("postgres") {
            BackendKind::Postgres
        } else if self.url.starts_with("mysql") || self.url.starts_with("mariadb") {
            BackendKind::Mysql
        } else if self.url.starts_with("sqlite") {
            BackendKind::Sqlite
        } else {
            BackendKind::Unknown
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendKind {
    Postgres,
    Mysql,
    Sqlite,
    Unknown,
}
