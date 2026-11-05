use std::future::Future;
use std::pin::Pin;

pub type HookResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
pub type BoxFuture<'a> = Pin<Box<dyn Future<Output = HookResult> + Send + 'a>>;

/// Optional lifecycle hooks for a Model.
/// All methods have a no-op default — implement only what you need.
/// Returning `Err` from any hook cancels the DB operation.
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
