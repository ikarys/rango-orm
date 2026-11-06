use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Result};

use crate::model::{extract_option, field_type_to_getter, type_name};
use crate::rango_core_path;

/// Expand `#[derive(Partial)]`.
pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let core = rango_core_path();

    let mut model: Option<syn::Path> = None;
    for a in input.attrs.iter().filter(|a| a.path().is_ident("partial")) {
        a.parse_nested_meta(|m| {
            if m.path.is_ident("model") {
                model = Some(m.value()?.parse()?);
                Ok(())
            } else {
                Err(m.error("expected `model = Path`"))
            }
        })?;
    }
    let model = model.ok_or_else(|| {
        syn::Error::new_spanned(name, "Partial requires `#[partial(model = Path)]`")
    })?;

    let Data::Struct(s) = &input.data else {
        return Err(syn::Error::new_spanned(
            name,
            "Partial can only be derived on structs",
        ));
    };
    let Fields::Named(fields) = &s.fields else {
        return Err(syn::Error::new_spanned(
            name,
            "Partial requires named fields",
        ));
    };

    let mut checks = Vec::new();
    let mut col_names = Vec::new();
    let mut from_row_fields = Vec::new();
    for f in &fields.named {
        let fname = f.ident.as_ref().unwrap();
        // Same constant the model derive generates: `User::EMAIL` for `email: Option<FieldEmail>`.
        let const_name = syn::Ident::new(
            &fname.to_string().trim_start_matches("r#").to_uppercase(),
            fname.span(),
        );
        let (nullable, inner_ty) = extract_option(&f.ty);
        // The column name comes from the model, so `#[field(column = "...")]` renames apply.
        let col = quote! { <#model>::#const_name.name() };
        checks.push(quote! { let _: #core::Column<#model, #inner_ty> = <#model>::#const_name; });
        col_names.push(col.clone());
        let type_str = type_name(inner_ty);
        let getter = field_type_to_getter(&type_str, &col, nullable, &core);
        from_row_fields.push(quote! { #fname: #getter });
    }

    Ok(quote! {
        impl #core::FromRow for #name {
            fn from_row(row: &dyn #core::RangoRow) -> Result<Self, #core::RowError> {
                Ok(Self {
                    #(#from_row_fields),*
                })
            }
        }

        impl #core::Partial for #name {
            type Model = #model;

            fn columns() -> &'static [&'static str] {
                #(#checks)*
                const COLUMNS: &[&str] = &[#(#col_names),*];
                COLUMNS
            }
        }
    })
}
