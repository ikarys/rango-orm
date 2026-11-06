//! `#[derive(Model)]` and `#[derive(ModelMixin)]` for [Rango ORM](https://crates.io/crates/rango-orm).
//!
//! Internal crate: depend on `rango-orm`, which re-exports the derives.
#![forbid(unsafe_code)]
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

mod implicit_pk;
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

/// Attribute macro: adds an implicit primary key, then derives [`Model`].
///
/// - no PK declared: injects `pub id: FieldUuid` as the first field
/// - `#[rango::model(pk = "bigint")]`: injects `id: FieldAutoBigInt` (database-generated);
///   `"int"` gives `FieldAutoInt`, `"uuid"` is the default
/// - a PK already exists (`id` field or `#[field(primary_key)]`): nothing is injected
/// - `#[derive(Model)]` is added unless the struct already has it
///
/// The injected type is a bare `FieldUuid` / `FieldAutoBigInt`: import it (e.g. `rango::prelude::*`).
/// The default is not read from `rango.toml`: cargo does not track that file,
/// so builds would go stale when it changes.
///
/// ```rust,ignore
/// #[rango::model]
/// struct Article { title: FieldVarchar<1, 200> }
/// ```
#[proc_macro_attribute]
pub fn model(args: TokenStream, input: TokenStream) -> TokenStream {
    implicit_pk::expand(args.into(), input.into())
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

/// Returns the path to the `Model` derive: through the `rango-orm` facade, or directly.
pub(crate) fn rango_derive_path() -> TokenStream2 {
    match proc_macro_crate::crate_name("rango-orm") {
        Ok(proc_macro_crate::FoundCrate::Itself) => quote! { ::rango::Model },
        Ok(proc_macro_crate::FoundCrate::Name(name)) => {
            let name = if name == "rango_orm" {
                "rango"
            } else {
                name.as_str()
            };
            let ident = proc_macro2::Ident::new(name, proc_macro2::Span::call_site());
            quote! { ::#ident::Model }
        }
        Err(_) => quote! { ::rango_orm_derive::Model },
    }
}
