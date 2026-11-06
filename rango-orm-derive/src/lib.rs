#![forbid(unsafe_code)]
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

mod mixin;
mod model;
mod utils;

/// Returns the path to rango_orm_core: through the `rango-orm` facade, or directly.
pub(crate) fn rango_core_path() -> TokenStream2 {
    match proc_macro_crate::crate_name("rango-orm") {
        Ok(proc_macro_crate::FoundCrate::Itself) => quote! { ::rango_orm_core },
        Ok(proc_macro_crate::FoundCrate::Name(name)) => {
            // The facade's lib is named `rango`; proc-macro-crate returns the
            // dependency key, which is `rango_orm` unless the user renamed it.
            let name = if name == "rango_orm" {
                "rango"
            } else {
                name.as_str()
            };
            let ident = proc_macro2::Ident::new(name, proc_macro2::Span::call_site());
            quote! { ::#ident::rango_orm_core }
        }
        // No facade: the user depends on rango-orm-core directly.
        Err(_) => quote! { ::rango_orm_core },
    }
}

/// Derive macro for Rango models.
///
/// Generates:
/// - `impl Model for MyStruct` with `table_name()` and `schema()`
/// - Table name defaults to snake_case of the struct name (e.g. `UserProfile` → `user_profile`)
///
/// # Example
/// ```rust,ignore
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
/// ```rust,ignore
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
