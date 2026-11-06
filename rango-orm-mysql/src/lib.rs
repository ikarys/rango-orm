//! MySQL 8.0+ / MariaDB 10.6+ backend for [Rango ORM](https://crates.io/crates/rango-orm).
//!
//! Internal crate: depend on `rango-orm` with feature `mysql`.
#![forbid(unsafe_code)]
use crate::error::db_err;
use rango_orm_core::config::is_local_host;
use std::str::FromStr;
use std::time::Duration;

pub use sqlx::MySqlPool;
use sqlx::{
    mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode},
    ConnectOptions,
};

pub use m2m::M2M;
pub use ops::{
    all, bulk_create, delete, get, get_or_create, insert, raw, raw_execute, raw_scalar, update,
};
pub use query_builder::{QueryBuilder, RangoFilterExt, SelectAs};
pub use rango_orm_core::WithRelated;
pub use transaction::{atomic, MySqlTransaction, TxFuture};

mod error;
pub mod m2m;
mod ops;
mod query_builder;
mod row;
mod transaction;

/// Newtype wrapper around MySqlPool that implements RangoBackend.
/// Use this when you need the backend trait — for direct DB calls use MySqlPool directly.
#[derive(Clone)]
pub struct RangoMySqlPool(pub MySqlPool);

impl std::ops::Deref for RangoMySqlPool {
    type Target = MySqlPool;
    fn deref(&self) -> &MySqlPool {
        &self.0
    }
}

impl rango_orm_core::RangoBackend for RangoMySqlPool {
    fn backend_kind(&self) -> rango_orm_core::BackendKind {
        rango_orm_core::BackendKind::Mysql
    }
    fn placeholder(&self, _n: usize) -> String {
        "?".to_string()
    }
}

/// Connect to MySQL 8.0+ / MariaDB 10.6+. The session runs in UTC (`DATETIME` has no time zone)
/// with strict SQL mode, so invalid values are rejected instead of silently truncated.
/// `mariadb://` URLs are accepted as an alias of `mysql://`.
///
/// TLS: an explicit `ssl-mode` in the URL always wins. Without one, remote hosts get `REQUIRED`
/// instead of sqlx's `PREFERRED`, which silently falls back to cleartext under a MITM.
/// Use `ssl-mode=VERIFY_IDENTITY` in production to also authenticate the server certificate.
pub async fn connect(url: &str) -> rango_orm_core::Result<MySqlPool> {
    let url = match url.strip_prefix("mariadb://") {
        Some(rest) => format!("mysql://{rest}"),
        None => url.to_string(),
    };
    let mut opts = MySqlConnectOptions::from_str(&url).map_err(db_err)?;
    let explicit_ssl = url.contains("ssl-mode=") || url.contains("sslmode=");
    if !explicit_ssl && opts.get_socket().is_none() && !is_local_host(opts.get_host()) {
        opts = opts.ssl_mode(MySqlSslMode::Required);
    }
    let opts = opts
        .charset("utf8mb4")
        .timezone(Some("+00:00".to_string()))
        .pipes_as_concat(false)
        .log_slow_statements(log::LevelFilter::Warn, Duration::from_millis(500));

    MySqlPoolOptions::new()
        .test_before_acquire(false)
        .after_connect(|conn, _| {
            Box::pin(async move {
                use sqlx::Executor;
                conn.execute("SET SESSION sql_mode = 'STRICT_ALL_TABLES,NO_ZERO_DATE,NO_ZERO_IN_DATE,ERROR_FOR_DIVISION_BY_ZERO,NO_ENGINE_SUBSTITUTION'")
                    .await?;
                Ok(())
            })
        })
        .connect_with(opts)
        .await
        .map_err(db_err)
}
