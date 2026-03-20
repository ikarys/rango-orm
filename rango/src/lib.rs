/// Rango ORM — re-exports everything the user needs.
///
/// Usage:
/// ```rust
/// use rango::prelude::*;
///
/// #[derive(Model)]
/// struct User {
///     id: FieldUuid,
///     email: FieldEmail,
/// }
/// ```

// Re-export core types
pub use rango_core::*;

// Make rango_core visible as a top-level crate for proc-macro generated code.
// This allows `::rango_core::` paths to resolve in downstream crates
// that only depend on `rango`.
#[doc(hidden)]
pub extern crate rango_core;

// Re-export the derive macro
pub use rango_derive::Model;
pub use rango_derive::ModelMixin;

// Re-export database pool type
pub use rango_postgres::PgPool;

/// Connect to a PostgreSQL database.
///
/// # Example
/// ```rust
/// let config = DatabaseConfig::from_env().unwrap();
/// let pool = rango::connect(&config).await?;
/// ```
pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    rango_postgres::connect(config).await
}

/// Convenience prelude — import everything with `use rango::prelude::*`
pub mod prelude {
    pub use rango_core::*;
    pub use rango_derive::{Model, ModelMixin};
    pub use rango_postgres::PgPool;
    pub use rango_core::ModelHooks;
}
