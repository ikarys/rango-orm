//! PostgreSQL backend for [Rango ORM](https://crates.io/crates/rango-orm).
//!
//! Internal crate: depend on `rango-orm` (feature `postgres`, on by default).
#![forbid(unsafe_code)]
mod error;
pub mod m2m;
mod ops;
pub mod pg_ext;
mod query_builder;
pub use rango_orm_core::related;
mod row;
mod transaction;

use rango_orm_core::DatabaseConfig;
use rango_orm_core::config::is_local_host;
use sqlx::{
    ConnectOptions, Executor,
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
};
use std::time::Duration;

pub use m2m::M2M;
pub use ops::{
    all, bulk_create, bulk_update, bulk_upsert, delete, get, get_or_create, insert, raw,
    raw_execute, raw_scalar, update, update_or_create,
};
pub use pg_ext::{Pg, PgQueryExt};
pub use query_builder::{Conn, QueryBuilder, SelectAs};
pub use rango_orm_core::WithRelated;
pub use sqlx::PgPool;
pub use transaction::{RangoTransaction, atomic, atomic_retry, transaction};

use rango_orm_core::{Filterable, FromRow, Model, ModelValues};

/// Extension trait — adds .filter(&pool) to any Model.
pub trait RangoFilterExt: Model + ModelValues + FromRow + Filterable + Sized {
    /// `&pool`, or `&mut **tx` inside `atomic`.
    fn filter<'a>(conn: impl Into<Conn<'a>>) -> QueryBuilder<'a, Self> {
        QueryBuilder::new(conn)
    }
}

/// Blanket impl — any Model that implements the required traits gets .filter() for free.
impl<M> RangoFilterExt for M where M: Model + ModelValues + FromRow + Filterable {}

pub use self::RangoFilterExt as FilterExt;

/// Parse the URL; remote hosts get `sslmode=require` unless the URL or `PGSSLMODE` sets one.
fn tls_default(url: &str, env_sslmode: bool) -> rango_orm_core::Result<PgConnectOptions> {
    let mut opts: PgConnectOptions = url.parse().map_err(error::db_err)?;
    if !(url.contains("sslmode=") || env_sslmode) && !is_local_host(opts.get_host()) {
        opts = opts.ssl_mode(PgSslMode::Require);
    }
    Ok(opts)
}

/// Connect to a PostgreSQL database and return a connection pool.
/// Applies `after_connect` SQL statements on every new connection if configured.
///
/// TLS: an explicit `sslmode` (URL or `PGSSLMODE`) always wins. Without one, remote hosts get
/// `require` instead of libpq's `prefer`, which silently falls back to cleartext under a MITM.
/// Use `sslmode=verify-full` in production to also authenticate the server certificate.
/// Timeouts and slow query logging are applied from `DatabaseConfig`; sqlx emits events on the `sqlx::query` tracing target.
pub async fn connect(config: &DatabaseConfig) -> rango_orm_core::Result<PgPool> {
    let after_connect = config.after_connect.clone();

    let mut opts = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(Duration::from_secs(config.connect_timeout))
        .idle_timeout(Duration::from_secs(config.idle_timeout))
        .max_lifetime(Duration::from_secs(config.max_lifetime))
        .test_before_acquire(config.test_before_acquire);

    if !after_connect.is_empty() {
        opts = opts.after_connect(move |conn, _meta| {
            let stmts = after_connect.clone();
            Box::pin(async move {
                for stmt in &stmts {
                    conn.execute(sqlx::AssertSqlSafe(stmt.as_str())).await?;
                }
                Ok(())
            })
        });
    }

    let mut connect_opts = tls_default(&config.url, std::env::var_os("PGSSLMODE").is_some())?;

    connect_opts = connect_opts.log_slow_statements(
        log::LevelFilter::Warn,
        Duration::from_millis(config.slow_query_threshold_ms),
    );

    if let Some(ms) = config.statement_timeout_ms {
        connect_opts = connect_opts.options([("statement_timeout", ms.to_string())]);
    }
    if let Some(ms) = config.lock_timeout_ms {
        connect_opts = connect_opts.options([("lock_timeout", ms.to_string())]);
    }

    opts.connect_with(connect_opts).await.map_err(error::db_err)
}

#[cfg(test)]
mod tls_tests {
    use super::*;

    #[test]
    fn remote_hosts_require_tls_unless_the_url_says_otherwise() {
        let mode = |url, env| tls_default(url, env).unwrap().get_ssl_mode();
        assert!(matches!(
            mode("postgres://u:p@db.example.com/d", false),
            PgSslMode::Require
        ));
        assert!(matches!(
            mode("postgres://u:p@localhost/d", false),
            PgSslMode::Prefer
        ));
        assert!(matches!(
            mode("postgres://u:p@127.0.0.1/d", false),
            PgSslMode::Prefer
        ));
        assert!(matches!(
            mode("postgres://u:p@db.example.com/d?sslmode=disable", false),
            PgSslMode::Disable
        ));
        assert!(matches!(
            mode("postgres://u:p@db.example.com/d", true),
            PgSslMode::Prefer
        ));
    }
}
