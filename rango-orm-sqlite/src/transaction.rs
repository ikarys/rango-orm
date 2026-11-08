use crate::error::db_err;
use rango_orm_core::Result;
use sqlx::{Sqlite, SqlitePool};

pub type SqliteTransaction<'a> = sqlx::Transaction<'a, Sqlite>;

/// Run `f` in a transaction: commit on `Ok`, roll back on `Err`.
pub async fn atomic<F, T>(pool: &SqlitePool, f: F) -> Result<T>
where
    F: AsyncFnOnce(&mut SqliteTransaction<'_>) -> Result<T>,
{
    let mut tx = pool.begin().await.map_err(db_err)?;
    match f(&mut tx).await {
        Ok(val) => {
            tx.commit().await.map_err(db_err)?;
            Ok(val)
        }
        Err(e) => {
            tx.rollback().await.ok();
            Err(e)
        }
    }
}
