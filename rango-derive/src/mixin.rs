use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Result};
use crate::rango_core_path;

/// Expand `#[derive(ModelMixin)]`.
pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let core = rango_core_path();

    Ok(quote! {
        impl #core::ModelMixin for #name {
            fn mixin_name() -> &'static str {
                stringify!(#name)
            }
        }
    })
}
