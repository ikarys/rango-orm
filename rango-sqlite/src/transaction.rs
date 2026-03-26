use anyhow::Result;
use sqlx::{SqlitePool, Sqlite};

pub type SqliteTransaction<'a> = sqlx::Transaction<'a, Sqlite>;

pub async fn atomic<F, Fut, T>(pool: &SqlitePool, f: F) -> Result<T>
where
    F: FnOnce(&mut SqliteTransaction<'_>) -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut tx = pool.begin().await?;
    match f(&mut tx).await {
        Ok(val) => { tx.commit().await?; Ok(val) }
        Err(e)  => { tx.rollback().await.ok(); Err(e) }
    }
}
