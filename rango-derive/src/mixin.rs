use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Result};

/// Expand `#[derive(ModelMixin)]`.
/// Generates a `RangoMixin` impl that exposes the field list
/// so `#[derive(Model)]` can flatten them into the parent schema.
pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;

    // ModelMixin generates NO table — just a marker impl.
    Ok(quote! {
        impl ::rango_core::ModelMixin for #name {
            fn mixin_name() -> &'static str {
                stringify!(#name)
            }
        }
    })
}
