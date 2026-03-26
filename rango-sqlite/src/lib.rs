pub use sqlx::SqlitePool;

pub use ops::{
    all, delete, get, get_or_create, insert, update, bulk_create,
    raw, raw_scalar, raw_execute,
};
pub use query_builder::{QueryBuilder, RangoFilterExt};
pub use transaction::{atomic, SqliteTransaction};

mod ops;
mod query_builder;
mod row;
mod transaction;

/// Newtype wrapper around SqlitePool that implements RangoBackend.
/// Use this when you need the backend trait — for direct DB calls use SqlitePool directly.
#[derive(Clone)]
pub struct RangoSqlitePool(pub SqlitePool);

impl std::ops::Deref for RangoSqlitePool {
    type Target = SqlitePool;
    fn deref(&self) -> &SqlitePool { &self.0 }
}

impl rango_core::RangoBackend for RangoSqlitePool {
    fn backend_kind(&self) -> rango_core::BackendKind { rango_core::BackendKind::Sqlite }
    fn placeholder(&self, _n: usize) -> String { "?".to_string() }
}

pub async fn connect(url: &str) -> anyhow::Result<SqlitePool> {
    SqlitePool::connect(url).await.map_err(|e| anyhow::anyhow!(e))
}
