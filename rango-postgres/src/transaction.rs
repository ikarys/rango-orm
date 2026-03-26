use anyhow::Result;
use sqlx::{PgPool, Postgres, Transaction};
use std::future::Future;

/// Type alias for a Rango/sqlx transaction — use this in function signatures
/// instead of the verbose `Transaction<'_, Postgres>`.
///
/// # Example
/// ```rust,ignore
/// async fn create_user_with_profile(tx: &mut RangoTransaction<'_>, ...) -> Result<User> {
///     let user = rango::insert(tx, ...).await?;
///     rango::insert(tx, profile).await?;
///     Ok(user)
/// }
///
/// rango::atomic(&pool, |tx| async move {
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
/// let user = rango::atomic(&pool, |tx| async move {
///     let user = rango::insert(tx, User { ... }).await?;
///     rango::insert(tx, Profile {
///         user_id: ForeignKey::new(user.id.0),
///         ...
///     }).await?;
///     Ok(user)
/// }).await?;
/// ```
pub async fn atomic<F, Fut, T>(pool: &PgPool, f: F) -> Result<T>
where
    F: FnOnce(&mut RangoTransaction<'_>) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to begin transaction: {}", e))?;

    match f(&mut tx).await {
        Ok(result) => {
            tx.commit()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to commit: {}", e))?;
            Ok(result)
        }
        Err(e) => {
            let _ = tx.rollback().await;
            Err(e)
        }
    }
}

/// Alias for [`atomic`] — same semantics, alternative name.
pub async fn transaction<F, Fut, T>(pool: &PgPool, f: F) -> Result<T>
where
    F: FnOnce(&mut RangoTransaction<'_>) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    atomic(pool, f).await
}
