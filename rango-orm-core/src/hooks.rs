use crate::{Error, Result};
use std::future::Future;
use std::pin::Pin;

pub type HookResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
pub type BoxFuture<'a> = Pin<Box<dyn Future<Output = HookResult> + Send + 'a>>;

/// Optional lifecycle hooks for a Model.
/// All methods have a no-op default — implement only what you need.
/// Returning `Err` from a `before_*` hook cancels the operation (no SQL runs); the error
/// reaches the caller as `Error::Hook`. An `after_*` error is returned too, but the statement
/// has already run: it is rolled back only if the caller is inside `atomic()` / a transaction.
///
/// Fired by `insert`, `update`, `delete`, `get_or_create` and `update_or_create`; never by
/// bulk operations, query-builder updates/deletes or raw SQL. `#[derive(Model)]` emits an empty
/// impl unless the struct has `#[model(hooks)]`, in which case you write `impl ModelHooks`.
pub trait ModelHooks: Sized {
    /// Before INSERT or UPDATE.
    fn before_save(&mut self) -> BoxFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    /// After INSERT or UPDATE.
    fn after_save(&self) -> BoxFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    /// Before INSERT only.
    fn before_create(&mut self) -> BoxFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    /// After INSERT only.
    fn after_create(&self) -> BoxFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    /// Before UPDATE only.
    fn before_update(&mut self) -> BoxFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    /// After UPDATE only.
    fn after_update(&self) -> BoxFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    /// Before DELETE.
    fn before_delete(&self) -> BoxFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    /// After DELETE.
    fn after_delete(&self) -> BoxFuture<'_> {
        Box::pin(async { Ok(()) })
    }
}

// ─── Ordering shared by every backend ─────────────────────────────────────────
// Backends call these around the SQL statement; a hook error becomes `Error::Hook`.

fn run(r: HookResult) -> Result<()> {
    r.map_err(Error::Hook)
}

#[doc(hidden)]
pub async fn before_insert<M: ModelHooks>(m: &mut M) -> Result<()> {
    run(m.before_save().await)?;
    run(m.before_create().await)
}

#[doc(hidden)]
pub async fn after_insert<M: ModelHooks>(m: &M) -> Result<()> {
    run(m.after_create().await)?;
    run(m.after_save().await)
}

#[doc(hidden)]
pub async fn before_update<M: ModelHooks>(m: &mut M) -> Result<()> {
    run(m.before_save().await)?;
    run(m.before_update().await)
}

#[doc(hidden)]
pub async fn after_update<M: ModelHooks>(m: &M) -> Result<()> {
    run(m.after_update().await)?;
    run(m.after_save().await)
}

#[doc(hidden)]
pub async fn before_delete<M: ModelHooks>(m: &M) -> Result<()> {
    run(m.before_delete().await)
}

#[doc(hidden)]
pub async fn after_delete<M: ModelHooks>(m: &M) -> Result<()> {
    run(m.after_delete().await)
}
