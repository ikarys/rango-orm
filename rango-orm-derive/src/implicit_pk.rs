//! `#[rango::model]`: injects an implicit `id` primary key, then derives `Model`.
use proc_macro2::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{Expr, ExprLit, Fields, ItemStruct, Lit, Meta, Token, parse::Parser, parse_quote};

use crate::rango_derive_path;

const USAGE: &str = "Rango: expected `pk = \"uuid\"`, `pk = \"bigint\"` or `pk = \"int\"`";

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let mut item: ItemStruct = syn::parse2(input)?;
    let mut pk = "uuid".to_string();
    for meta in Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)? {
        match &meta {
            Meta::NameValue(nv)
                if nv.path.is_ident("pk")
                    && let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = &nv.value =>
            {
                pk = s.value();
            }
            _ => return Err(syn::Error::new_spanned(meta, USAGE)),
        }
    }
    let ty = match pk.as_str() {
        "uuid" => "FieldUuid",
        "bigint" => "FieldAutoBigInt",
        "int" => "FieldAutoInt",
        other => {
            return Err(syn::Error::new_spanned(
                &item.ident,
                format!(
                    "Rango: unknown pk type `{other}`, expected \"uuid\", \"bigint\" or \"int\""
                ),
            ));
        }
    };

    let Fields::Named(fields) = &mut item.fields else {
        return Err(syn::Error::new_spanned(
            &item.ident,
            "Rango: models need named fields",
        ));
    };
    let has_pk = fields.named.iter().any(|f| {
        f.ident.as_ref().is_some_and(|i| i == "id")
            || f.attrs.iter().any(|a| {
                a.path().is_ident("field")
                    && a.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                        .is_ok_and(|l| l.iter().any(|m| m.path().is_ident("primary_key")))
            })
    });
    if !has_pk {
        let ty = proc_macro2::Ident::new(ty, proc_macro2::Span::call_site());
        let vis = fields
            .named
            .first()
            .map(|f| f.vis.clone())
            .unwrap_or_else(|| parse_quote!(pub));
        let field = syn::Field::parse_named.parse2(quote! { #vis id: #ty })?;
        fields.named.insert(0, field);
    }

    let has_derive = item.attrs.iter().any(|a| {
        a.path().is_ident("derive")
            && a.parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
                .is_ok_and(|l| {
                    l.iter()
                        .any(|p| p.segments.last().is_some_and(|s| s.ident == "Model"))
                })
    });
    if !has_derive {
        let d = rango_derive_path();
        item.attrs.insert(0, parse_quote!(#[derive(#d)]));
    }
    Ok(quote!(#item))
}
