mod ops;
mod query_builder;
mod row;

use rango_core::DatabaseConfig;
use sqlx::{postgres::PgPoolOptions, Executor};
use std::time::Duration;

pub use sqlx::PgPool;
pub use ops::{all, delete, get, get_or_create, insert, update, update_or_create};
pub(crate) use row::PgRangoRow2;
pub use query_builder::QueryBuilder;

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
