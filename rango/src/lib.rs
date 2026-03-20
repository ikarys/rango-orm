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

// Re-export the derive macro
pub use rango_derive::Model;
pub use rango_derive::ModelMixin;

/// Convenience prelude — import everything with `use rango::prelude::*`
pub mod prelude {
    pub use rango_core::*;
    pub use rango_derive::{Model, ModelMixin};
}
