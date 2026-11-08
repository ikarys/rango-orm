use crate::error::db_err;
use rango_orm_core::Result;
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Duration;

/// Type alias for a Rango/sqlx transaction — use this in function signatures
/// instead of the verbose `Transaction<'_, Postgres>`.
///
/// # Example
/// ```rust,ignore
/// async fn create_user_with_profile(tx: &mut RangoTransaction<'_>, ...) -> Result<User> {
///     let user = rango::insert(&mut **tx, ...).await?;
///     rango::insert(&mut **tx, profile).await?;
///     Ok(user)
/// }
///
/// rango::atomic(&pool, async |tx| {
///     create_user_with_profile(tx, ...).await
/// }).await?;
/// ```
pub type RangoTransaction<'a> = Transaction<'a, Postgres>;

/// Execute a closure atomically — commits on Ok, rolls back on Err.
///
/// All Rango ops (`insert`, `update`, `delete`, `get`, `all`) accept both `&PgPool`
/// and `&mut Transaction` — no separate `_tx` variants needed.
/// For compound ops (`get_or_create`, `update_or_create`), pass `tx` directly.
///
/// # Example
/// ```rust,ignore
/// let user = rango::atomic(&pool, async |tx| {
///     let user = rango::insert(&mut **tx, User { ... }).await?;
///     rango::insert(&mut **tx, Profile {
///         user_id: ForeignKey::new(user.id.0),
///         ...
///     }).await?;
///     Ok(user)
/// }).await?;
/// ```
pub async fn atomic<F, T>(pool: &PgPool, f: F) -> Result<T>
where
    F: AsyncFnOnce(&mut RangoTransaction<'_>) -> Result<T>,
{
    let mut tx = pool.begin().await.map_err(db_err)?;

    match f(&mut tx).await {
        Ok(result) => {
            tx.commit().await.map_err(db_err)?;
            Ok(result)
        }
        Err(e) => {
            let _ = tx.rollback().await;
            Err(e)
        }
    }
}

/// Execute a closure atomically, retrying on `Error::Conflict` (serialization
/// failures and deadlocks).
///
/// Use this for `SERIALIZABLE` / `REPEATABLE READ` workloads and deadlock-prone
/// transactions. The closure may run several times, so it must not have side
/// effects outside the transaction (no emails, no HTTP calls).
///
/// `max_attempts` counts total runs; `0` is treated as `1`.
///
/// # Example
/// ```rust,ignore
/// let n = rango::atomic_retry(&pool, 3, async |tx| {
///     rango::raw_execute(
///         &mut **tx,
///         "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
///         vec![],
///     )
///     .await?;
///     ...
/// }).await?;
/// ```
pub async fn atomic_retry<F, T>(pool: &PgPool, max_attempts: u32, mut f: F) -> Result<T>
where
    F: AsyncFnMut(&mut RangoTransaction<'_>) -> Result<T>,
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
pub async fn transaction<F, T>(pool: &PgPool, f: F) -> Result<T>
where
    F: AsyncFnOnce(&mut RangoTransaction<'_>) -> Result<T>,
{
    atomic(pool, f).await
}
