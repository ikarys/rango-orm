use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Data, DeriveInput, Field, Fields, Lit, Meta, Result, Type,
    Token,
    punctuated::Punctuated,
};

use crate::utils::to_snake_case;

/// Parsed `#[field(...)]` attributes for one field.
#[derive(Default)]
struct FieldAttr {
    unique: bool,
    index: bool,
    primary_key: bool,
    auto_now_add: bool,
    auto_now: bool,
    column: Option<String>,
    default: Option<String>,
    comment: Option<String>,
}

/// Parsed `#[model(...)]` attributes on the struct.
#[derive(Default)]
struct ModelAttr {
    table: Option<String>,
    comment: Option<String>,
}

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let struct_name = &input.ident;
    let model_attr = parse_model_attr(&input)?;

    // Table name: explicit override or snake_case of struct name
    let table_name = model_attr
        .table
        .unwrap_or_else(|| to_snake_case(&struct_name.to_string()));

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => return Err(syn::Error::new_spanned(struct_name, "Model requires named fields")),
        },
        _ => return Err(syn::Error::new_spanned(struct_name, "Model can only be derived on structs")),
    };

    // Generate one ColumnDef per field
    let column_defs: Vec<TokenStream> = fields
        .iter()
        .map(|f| generate_column_def(f))
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        impl ::rango_core::Model for #struct_name {
            fn table_name() -> &'static str {
                #table_name
            }

            fn schema() -> ::rango_core::TableSchema {
                ::rango_core::TableSchema {
                    table_name: #table_name.to_string(),
                    columns: vec![
                        #(#column_defs),*
                    ],
                }
            }
        }
    })
}

/// Parse `#[model(table = "...", comment = "...")]`
fn parse_model_attr(input: &DeriveInput) -> Result<ModelAttr> {
    let mut attr = ModelAttr::default();
    for a in &input.attrs {
        if !a.path().is_ident("model") {
            continue;
        }
        let nested = a.parse_args_with(
            Punctuated::<Meta, Token![,]>::parse_terminated
        )?;
        for meta in nested {
            match meta {
                Meta::NameValue(nv) if nv.path.is_ident("table") => {
                    if let syn::Expr::Lit(expr_lit) = &nv.value {
                        if let Lit::Str(s) = &expr_lit.lit {
                            attr.table = Some(s.value());
                        }
                    }
                }
                Meta::NameValue(nv) if nv.path.is_ident("comment") => {
                    if let syn::Expr::Lit(expr_lit) = &nv.value {
                        if let Lit::Str(s) = &expr_lit.lit {
                            attr.comment = Some(s.value());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(attr)
}

/// Parse `#[field(unique, index, primary_key, auto_now_add, auto_now, column="...", default="...", comment="...")]`
fn parse_field_attr(field: &Field) -> Result<FieldAttr> {
    let mut attr = FieldAttr::default();
    for a in &field.attrs {
        if !a.path().is_ident("field") {
            continue;
        }
        let nested = a.parse_args_with(
            Punctuated::<Meta, Token![,]>::parse_terminated
        )?;
        for meta in nested {
            match &meta {
                Meta::Path(p) if p.is_ident("unique")       => attr.unique = true,
                Meta::Path(p) if p.is_ident("index")        => attr.index = true,
                Meta::Path(p) if p.is_ident("primary_key")  => attr.primary_key = true,
                Meta::Path(p) if p.is_ident("auto_now_add") => attr.auto_now_add = true,
                Meta::Path(p) if p.is_ident("auto_now")     => attr.auto_now = true,
                Meta::NameValue(nv) if nv.path.is_ident("column") => {
                    if let syn::Expr::Lit(expr_lit) = &nv.value {
                        if let Lit::Str(s) = &expr_lit.lit { attr.column = Some(s.value()); }
                    }
                }
                Meta::NameValue(nv) if nv.path.is_ident("default") => {
                    if let syn::Expr::Lit(expr_lit) = &nv.value {
                        if let Lit::Str(s) = &expr_lit.lit { attr.default = Some(s.value()); }
                    }
                }
                Meta::NameValue(nv) if nv.path.is_ident("comment") => {
                    if let syn::Expr::Lit(expr_lit) = &nv.value {
                        if let Lit::Str(s) = &expr_lit.lit { attr.comment = Some(s.value()); }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(attr)
}

/// Generate a `ColumnDef { ... }` expression for one struct field.
fn generate_column_def(field: &Field) -> Result<TokenStream> {
    let field_name = field.ident.as_ref().unwrap();
    let attr = parse_field_attr(field)?;

    // Column name: explicit override or snake_case field name
    let col_name = attr.column.unwrap_or_else(|| field_name.to_string());

    // Detect nullable: if the type is Option<T>
    let (nullable, inner_ty) = extract_option(&field.ty);

    // Auto-detect primary key: field named "id" gets PK by default
    let is_pk = attr.primary_key || field_name == "id";
    let is_unique = is_pk || attr.unique; // PK implies unique

    // Map Rust field type to ColumnType
    let col_type = map_field_type(inner_ty)?;

    // Default value
    let default_val = if attr.auto_now_add || attr.auto_now {
        quote! { Some(::rango_core::DefaultValue::CurrentTimestamp) }
    } else if let Some(d) = attr.default {
        quote! { Some(::rango_core::DefaultValue::Literal(#d.to_string())) }
    } else {
        quote! { None }
    };

    Ok(quote! {
        ::rango_core::ColumnDef {
            name: #col_name.to_string(),
            col_type: #col_type,
            nullable: #nullable,
            primary_key: #is_pk,
            unique: #is_unique,
            default: #default_val,
            references: None,
        }
    })
}

/// Returns (is_nullable, inner_type).
/// `Option<T>` → (true, T), anything else → (false, original_type)
fn extract_option(ty: &Type) -> (bool, &Type) {
    if let Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            if seg.ident == "Option" {
                if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        return (true, inner);
                    }
                }
            }
        }
    }
    (false, ty)
}

/// Map a Rango field type path to a `ColumnType` token.
fn map_field_type(ty: &Type) -> Result<TokenStream> {
    let type_str = quote!(#ty).to_string().replace(" ", "");

    let col_type = match type_str.as_str() {
        "FieldBool"                 => quote! { ::rango_core::ColumnType::Bool },
        "FieldSmallInt"             => quote! { ::rango_core::ColumnType::SmallInt },
        "FieldInt"                  => quote! { ::rango_core::ColumnType::Int },
        "FieldBigInt"               => quote! { ::rango_core::ColumnType::BigInt },
        "FieldFloat"                => quote! { ::rango_core::ColumnType::Float },
        "FieldDouble"               => quote! { ::rango_core::ColumnType::Double },
        "FieldText"                 => quote! { ::rango_core::ColumnType::Text },
        "FieldEmail"                => quote! { ::rango_core::ColumnType::Varchar(254) },
        "FieldUrl"                  => quote! { ::rango_core::ColumnType::Varchar(2048) },
        "FieldBytes"                => quote! { ::rango_core::ColumnType::Bytea },
        "FieldUuid"                 => quote! { ::rango_core::ColumnType::Uuid },
        "FieldDate"                 => quote! { ::rango_core::ColumnType::Date },
        "FieldTime"                 => quote! { ::rango_core::ColumnType::Time },
        "FieldDateTime"             => quote! { ::rango_core::ColumnType::DateTime },
        "FieldJson"                 => quote! { ::rango_core::ColumnType::Jsonb },
        s if s.starts_with("FieldVarchar<") => {
            // FieldVarchar<MIN, MAX> → we only need MAX for the SQL type
            parse_varchar(s)?
        }
        s if s.starts_with("FieldDecimal<") => {
            parse_decimal(s)?
        }
        s if s.starts_with("FieldRange<") => {
            // FieldRange stores as the base integer type
            quote! { ::rango_core::ColumnType::BigInt }
        }
        s if s.starts_with("FieldPassword<") => {
            parse_password(s)?
        }
        _ => {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("Unknown Rango field type: `{}`. Use a FieldXxx type.", type_str),
            ))
        }
    };
    Ok(col_type)
}

fn parse_varchar(s: &str) -> Result<TokenStream> {
    // FieldVarchar<MIN,MAX> — extract MAX
    let inner = s.trim_start_matches("FieldVarchar<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 {
        if let Ok(max) = parts[1].trim().parse::<u32>() {
            return Ok(quote! { ::rango_core::ColumnType::Varchar(#max) });
        }
    }
    Err(syn::Error::new(proc_macro2::Span::call_site(),
        format!("Invalid FieldVarchar syntax: `{}`", s)))
}

fn parse_decimal(s: &str) -> Result<TokenStream> {
    let inner = s.trim_start_matches("FieldDecimal<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 {
        if let (Ok(p), Ok(sc)) = (parts[0].trim().parse::<u8>(), parts[1].trim().parse::<u8>()) {
            return Ok(quote! { ::rango_core::ColumnType::Decimal { precision: #p, scale: #sc } });
        }
    }
    Err(syn::Error::new(proc_macro2::Span::call_site(),
        format!("Invalid FieldDecimal syntax: `{}`", s)))
}

fn parse_password(s: &str) -> Result<TokenStream> {
    // FieldPassword<MIN, MAX> → VARCHAR(MAX)
    let inner = s.trim_start_matches("FieldPassword<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 {
        if let Ok(max) = parts[1].trim().parse::<u32>() {
            return Ok(quote! { ::rango_core::ColumnType::Varchar(#max) });
        }
    }
    Err(syn::Error::new(proc_macro2::Span::call_site(),
        format!("Invalid FieldPassword syntax: `{}`", s)))
}
