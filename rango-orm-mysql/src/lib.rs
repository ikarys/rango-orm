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
    all, bulk_create, bulk_update, bulk_upsert, delete, get, get_or_create, insert, raw,
    raw_execute, raw_scalar, update, update_or_create,
};
pub use query_builder::{Conn, QueryBuilder, RangoFilterExt, SelectAs};
pub use rango_orm_core::WithRelated;
pub use transaction::{atomic, atomic_retry, transaction, MySqlTransaction};

mod error;
pub mod m2m;
mod ops;
mod query_builder;
mod row;
mod transaction;

/// Parse the URL; remote hosts get `ssl-mode=REQUIRED` unless the URL sets a mode.
fn tls_default(url: &str) -> rango_orm_core::Result<MySqlConnectOptions> {
    let mut opts = MySqlConnectOptions::from_str(url).map_err(db_err)?;
    let explicit_ssl = url.contains("ssl-mode=") || url.contains("sslmode=");
    if !explicit_ssl && opts.get_socket().is_none() && !is_local_host(opts.get_host()) {
        opts = opts.ssl_mode(MySqlSslMode::Required);
    }
    Ok(opts)
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
    let opts = tls_default(&url)?;
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

#[cfg(test)]
mod tls_tests {
    use super::*;

    #[test]
    fn remote_hosts_require_tls_unless_the_url_says_otherwise() {
        let mode = |url| tls_default(url).unwrap().get_ssl_mode();
        assert!(matches!(
            mode("mysql://u:p@db.example.com/d"),
            MySqlSslMode::Required
        ));
        assert!(matches!(
            mode("mysql://u:p@localhost/d"),
            MySqlSslMode::Preferred
        ));
        assert!(matches!(
            mode("mysql://u:p@127.0.0.1/d"),
            MySqlSslMode::Preferred
        ));
        assert!(matches!(
            mode("mysql://u:p@db.example.com/d?ssl-mode=disabled"),
            MySqlSslMode::Disabled
        ));
    }
}
