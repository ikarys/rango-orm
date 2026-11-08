//! SQLite backend for [Rango ORM](https://crates.io/crates/rango-orm).
//!
//! Internal crate: depend on `rango-orm` with feature `sqlite`.
#![forbid(unsafe_code)]
use crate::error::db_err;
use std::str::FromStr;
use std::time::Duration;

pub use sqlx::SqlitePool;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    ConnectOptions,
};

pub use m2m::M2M;
pub use ops::{
    all, bulk_create, bulk_update, bulk_upsert, delete, get, get_or_create, insert, raw,
    raw_execute, raw_scalar, update, update_or_create,
};
pub use query_builder::{Conn, QueryBuilder, RangoFilterExt, SelectAs};
pub use rango_orm_core::WithRelated;
pub use sqlite_ext::SqliteQueryExt;
pub use transaction::{atomic, transaction, SqliteTransaction};

mod error;
pub mod m2m;
mod ops;
mod query_builder;
mod row;
mod sqlite_ext;
mod transaction;

/// Connect with WAL (concurrent readers, one writer), `synchronous=NORMAL`, a 5s busy timeout to avoid SQLITE_BUSY, and foreign keys.
pub async fn connect(url: &str) -> rango_orm_core::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(url)
        .map_err(db_err)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true)
        .log_slow_statements(log::LevelFilter::Warn, Duration::from_millis(500));

    SqlitePoolOptions::new()
        .test_before_acquire(false)
        .connect_with(opts)
        .await
        .map_err(db_err)
}
