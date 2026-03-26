pub mod executor;
pub mod m2m;
mod ops;
pub mod pg_ext;
mod query_builder;
pub mod related;
mod row;
mod transaction;

use rango_core::DatabaseConfig;
use sqlx::{Executor, postgres::PgPoolOptions};
use std::time::Duration;

pub use executor::RangoExecutor;
pub use m2m::M2M;
pub use ops::{
    all, bulk_create, bulk_update, bulk_upsert, delete, get, get_or_create, insert, raw,
    raw_execute, raw_scalar, update, update_or_create,
};
pub use pg_ext::{Pg, PgQueryExt};
pub use query_builder::QueryBuilder;
pub use related::WithRelated;
pub use sqlx::PgPool;
pub use transaction::{RangoTransaction, atomic, transaction};

use rango_core::{Filterable, FromRow, Model, ModelValues};

/// Extension trait — adds .filter(&pool) to any Model.
pub trait RangoFilterExt: Model + ModelValues + FromRow + Filterable + Sized {
    fn filter(pool: &PgPool) -> QueryBuilder<Self> {
        QueryBuilder::new(pool)
    }
}

/// Blanket impl — any Model that implements the required traits gets .filter() for free.
impl<M> RangoFilterExt for M where M: Model + ModelValues + FromRow + Filterable {}

pub use self::RangoFilterExt as FilterExt;

/// Connect to a PostgreSQL database and return a connection pool.
/// Applies `after_connect` SQL statements on every new connection if configured.
pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    let after_connect = config.after_connect.clone();

    let mut opts = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(Duration::from_secs(config.connect_timeout))
        .idle_timeout(Duration::from_secs(config.idle_timeout));

    if !after_connect.is_empty() {
        opts = opts.after_connect(move |conn, _meta| {
            let stmts = after_connect.clone();
            Box::pin(async move {
                for stmt in &stmts {
                    conn.execute(stmt.as_str()).await?;
                }
                Ok(())
            })
        });
    }

    opts.connect(&config.url).await
}
