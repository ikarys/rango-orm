use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod model;
mod mixin;
mod utils;

/// Derive macro for Rango models.
///
/// Generates:
/// - `impl Model for MyStruct` with `table_name()` and `schema()`
/// - Table name defaults to snake_case of the struct name (e.g. `UserProfile` → `user_profile`)
///
/// # Example
/// ```rust
/// #[derive(Model)]
/// struct Article {
///     id: FieldUuid,
///     title: FieldVarchar<1, 255>,
///     body: Option<FieldText>,
///     #[field(index)]
///     slug: FieldVarchar<1, 100>,
///     #[field(auto_now_add)]
///     created_at: FieldDateTime,
/// }
/// ```
#[proc_macro_derive(Model, attributes(model, field))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    model::expand(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Marker derive for abstract model mixins (never creates a table).
///
/// # Example
/// ```rust
/// #[derive(ModelMixin)]
/// struct Timestamps {
///     #[field(auto_now_add)]
///     created_at: FieldDateTime,
///     #[field(auto_now)]
///     updated_at: FieldDateTime,
/// }
///
/// #[derive(Model)]
/// #[model(mixins(Timestamps))]
/// struct Article {
///     id: FieldUuid,
///     title: FieldVarchar<1, 255>,
/// }
/// ```
#[proc_macro_derive(ModelMixin, attributes(field))]
pub fn derive_model_mixin(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    mixin::expand(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
