/// Database connection configuration.
/// Backend-agnostic — works for PostgreSQL, MySQL, MariaDB, SQLite.
#[derive(Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout: u64,
    pub idle_timeout: u64,
    /// Close connections older than this many seconds (lets the DB rebalance, frees server memory).
    pub max_lifetime: u64,
    /// Ping each connection before handing it out. Off by default: saves one round-trip per
    /// acquire; enable behind proxies or failover setups that kill idle connections silently.
    pub test_before_acquire: bool,
    /// Postgres `statement_timeout` in milliseconds, set at connection startup. `None` = server default.
    pub statement_timeout_ms: Option<u64>,
    /// Postgres `lock_timeout` in milliseconds, set at connection startup. `None` = server default.
    pub lock_timeout_ms: Option<u64>,
    /// Queries slower than this (milliseconds) are logged at WARN (SQL text only, never bound values).
    pub slow_query_threshold_ms: u64,
    /// SQL statements executed on every new connection.
    /// Useful for strict mode, timezone, charset, search_path, etc.
    pub after_connect: Vec<String>,
}

impl std::fmt::Debug for DatabaseConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseConfig")
            .field("url", &Self::redact_url(&self.url))
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("connect_timeout", &self.connect_timeout)
            .field("idle_timeout", &self.idle_timeout)
            .field("max_lifetime", &self.max_lifetime)
            .field("test_before_acquire", &self.test_before_acquire)
            .field("statement_timeout_ms", &self.statement_timeout_ms)
            .field("lock_timeout_ms", &self.lock_timeout_ms)
            .field("slow_query_threshold_ms", &self.slow_query_threshold_ms)
            .field("after_connect", &self.after_connect)
            .finish()
    }
}

impl DatabaseConfig {
    pub fn redact_url(url: &str) -> String {
        let scheme_end = match url.find("://") {
            Some(idx) => idx,
            None => return url.to_string(),
        };
        let after_scheme = &url[scheme_end + 3..];
        // Userinfo lives in the authority (before the path/query); the last '@' ends it.
        let authority_end = after_scheme
            .find(['/', '?', '#'])
            .unwrap_or(after_scheme.len());
        let at_idx = match after_scheme[..authority_end].rfind('@') {
            Some(idx) => idx,
            None => return url.to_string(),
        };
        let authority = &after_scheme[..at_idx];
        let colon_idx = match authority.find(':') {
            Some(idx) => idx,
            None => return url.to_string(),
        };
        let user = &authority[..colon_idx];
        format!(
            "{}://{}:***@{}",
            &url[..scheme_end],
            user,
            &after_scheme[at_idx + 1..]
        )
    }

    /// Build from a full connection URL.
    /// Examples:
    ///   postgres://user:pass@localhost:5432/mydb
    ///   mysql://user:pass@localhost:3306/mydb
    ///   sqlite:///path/to/db.sqlite3
    pub fn from_url(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            max_connections: 20,
            min_connections: 2,
            connect_timeout: 10,
            idle_timeout: 600,
            max_lifetime: 1800,
            test_before_acquire: false,
            statement_timeout_ms: None,
            lock_timeout_ms: None,
            slow_query_threshold_ms: 500,
            after_connect: Vec::new(),
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Postgres,
    Mysql,
    Sqlite,
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_url_replaces_password() {
        assert_eq!(
            DatabaseConfig::redact_url("postgres://user:pass@host:5432/db"),
            "postgres://user:***@host:5432/db"
        );
    }

    #[test]
    fn redact_url_handles_special_chars_and_query() {
        assert_eq!(
            DatabaseConfig::redact_url("postgres://u:p@ss:w@host/db"),
            "postgres://u:***@host/db"
        );
        assert_eq!(
            DatabaseConfig::redact_url("postgres://host:5432/db?x=a@b"),
            "postgres://host:5432/db?x=a@b"
        );
    }

    #[test]
    fn redact_url_without_password_is_unchanged() {
        assert_eq!(
            DatabaseConfig::redact_url("postgres://user@host:5432/db"),
            "postgres://user@host:5432/db"
        );
    }

    #[test]
    fn redact_url_sqlite_memory_is_unchanged() {
        assert_eq!(
            DatabaseConfig::redact_url("sqlite::memory:"),
            "sqlite::memory:"
        );
    }

    #[test]
    fn database_config_debug_redacts_url() {
        let config = DatabaseConfig::from_url("postgres://user:secret@host:5432/db");
        let debug = format!("{:?}", config);
        assert!(!debug.contains("secret"));
        assert!(debug.contains("user:***@host"));
    }

    #[test]
    fn from_url_defaults() {
        let config = DatabaseConfig::from_url("postgres://user:pass@host:5432/db");
        assert_eq!(config.max_lifetime, 1800);
        assert!(!config.test_before_acquire);
        assert_eq!(config.statement_timeout_ms, None);
        assert_eq!(config.lock_timeout_ms, None);
        assert_eq!(config.slow_query_threshold_ms, 500);
    }
}
