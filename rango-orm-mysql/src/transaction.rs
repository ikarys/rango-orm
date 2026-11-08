use crate::error::db_err;
use rango_orm_core::Result;
use sqlx::{MySql, MySqlPool};
use std::time::Duration;

pub type MySqlTransaction<'a> = sqlx::Transaction<'a, MySql>;

/// Run `f` in a transaction: commit on `Ok`, roll back on `Err`.
pub async fn atomic<F, T>(pool: &MySqlPool, f: F) -> Result<T>
where
    F: AsyncFnOnce(&mut MySqlTransaction<'_>) -> Result<T>,
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

/// Execute a closure atomically, retrying on `Error::Conflict` (InnoDB deadlock,
/// error 1213, and lock wait timeout, error 1205).
///
/// The closure may run several times, so it must not have side effects outside the
/// transaction (no emails, no HTTP calls).
///
/// `max_attempts` counts total runs; `0` is treated as `1`.
///
/// # Example
/// ```rust,ignore
/// let n = rango::mysql::atomic_retry(&pool, 3, async |tx| {
///     rango::mysql::raw_execute(&mut **tx, "UPDATE ...", vec![]).await
/// }).await?;
/// ```
pub async fn atomic_retry<F, T>(pool: &MySqlPool, max_attempts: u32, mut f: F) -> Result<T>
where
    F: AsyncFnMut(&mut MySqlTransaction<'_>) -> Result<T>,
{
    let max_attempts = max_attempts.max(1);
    let mut attempt = 0u32;

    loop {
        attempt += 1;

        match atomic(pool, &mut f).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if !e.is_retryable() || attempt >= max_attempts {
                    return Err(e);
                }

                let shift = attempt.min(6);
                // ponytail: fixed exponential backoff without jitter; add jitter if contended workloads retry in lockstep.
                tokio::time::sleep(Duration::from_millis(5u64 << shift)).await;
            }
        }
    }
}

/// Alias for [`atomic`] — same semantics, alternative name.
pub async fn transaction<F, T>(pool: &MySqlPool, f: F) -> Result<T>
where
    F: AsyncFnOnce(&mut MySqlTransaction<'_>) -> Result<T>,
{
    atomic(pool, f).await
}
