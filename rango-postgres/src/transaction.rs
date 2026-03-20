use sqlx::{PgPool, Postgres, Transaction};
use anyhow::Result;
use std::future::Future;

/// Execute a closure atomically — commits on Ok, rolls back on Err.
///
/// All Rango ops (`insert`, `update`, `delete`) accept both `&PgPool`
/// and `&mut Transaction` — no separate `insert_tx` needed.
///
/// # Example
/// ```rust
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
    F: FnOnce(&mut Transaction<'_, Postgres>) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut tx = pool.begin().await
        .map_err(|e| anyhow::anyhow!("Failed to begin transaction: {}", e))?;

    match f(&mut tx).await {
        Ok(result) => {
            tx.commit().await
                .map_err(|e| anyhow::anyhow!("Failed to commit: {}", e))?;
            Ok(result)
        }
        Err(e) => {
            let _ = tx.rollback().await;
            Err(e)
        }
    }
}
