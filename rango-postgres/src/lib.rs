use rango_core::DatabaseConfig;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

pub use sqlx::PgPool;

/// Connect to a PostgreSQL database and return a connection pool.
pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(600))
        .connect(&config.url)
        .await
}
