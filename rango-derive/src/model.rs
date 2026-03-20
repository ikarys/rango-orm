use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Data, DeriveInput, Field, Fields, Lit, Meta, Result, Type,
    Token,
    punctuated::Punctuated,
};

use crate::utils::to_snake_case;
use crate::rango_core_path;

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

    let core = rango_core_path();

    // Generate one ColumnDef per field (skip ManyToMany — not a DB column)
    let column_defs: Vec<TokenStream> = fields
        .iter()
        .filter(|f| !is_many_to_many(&f.ty))
        .map(|f| generate_column_def(f, &core))
        .collect::<Result<Vec<_>>>()?;

    // Generate ModelValues impl (skip ManyToMany)
    let mut field_value_entries = Vec::new();
    let mut pk_value_expr = quote! { #core::SqlValue::Null };
    let mut pk_col_name = "id".to_string();

    for f in fields.iter() {
        if is_many_to_many(&f.ty) { continue; }
        let fname = f.ident.as_ref().unwrap();
        let is_pk = fname == "id" || parse_field_attr(f)?.primary_key;
        let col_name = fname.to_string();

        if is_pk {
            pk_col_name = col_name.clone();
            pk_value_expr = quote! {
                #core::ToSqlValue::to_sql_value(&self.#fname)
            };
        } else {
            field_value_entries.push(quote! {
                (#col_name, #core::ToSqlValue::to_sql_value(&self.#fname))
            });
        }
    }

    // Generate FromRow impl (ManyToMany fields are always empty — loaded separately)
    let mut from_row_fields = Vec::new();
    for f in fields.iter() {
        if is_many_to_many(&f.ty) {
            let fname = f.ident.as_ref().unwrap();
            from_row_fields.push(quote! { #fname: #core::ManyToMany::empty() });
            continue;
        }
        let fname = f.ident.as_ref().unwrap();
        let col_name = fname.to_string();
        let (nullable, inner_ty) = extract_option(&f.ty);
        let type_str = quote!(#inner_ty).to_string().replace(" ", "");

        let getter = field_type_to_getter(&type_str, &col_name, nullable, &core);
        from_row_fields.push(quote! { #fname: #getter });
    }

    let core2 = rango_core_path();
    Ok(quote! {
        impl #core2::Model for #struct_name {
            fn table_name() -> &'static str {
                #table_name
            }

            fn schema() -> #core2::TableSchema {
                #core2::TableSchema {
                    table_name: #table_name.to_string(),
                    columns: vec![
                        #(#column_defs),*
                    ],
                }
            }
        }

        impl #core2::FromRow for #struct_name {
            fn from_row(row: &dyn #core2::RangoRow) -> Result<Self, #core2::RowError> {
                Ok(Self {
                    #(#from_row_fields),*
                })
            }
        }

        impl #core2::Filterable for #struct_name {}

        impl #core2::ModelValues for #struct_name {
            fn field_values(&self) -> Vec<(&'static str, #core2::SqlValue)> {
                vec![#(#field_value_entries),*]
            }

            fn pk_value(&self) -> #core2::SqlValue {
                #pk_value_expr
            }

            fn pk_column() -> &'static str {
                #pk_col_name
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
fn generate_column_def(field: &Field, core: &TokenStream) -> Result<TokenStream> {
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
    let col_type = map_field_type(inner_ty, core)?;

    // Validate: auto_now_add / auto_now only on date/time fields
    if attr.auto_now_add || attr.auto_now {
        let type_str = quote!(#inner_ty).to_string().replace(" ", "");
        if !matches!(type_str.as_str(), "FieldDate" | "FieldTime" | "FieldDateTime") {
            return Err(syn::Error::new_spanned(
                field,
                "#[field(auto_now_add)] and #[field(auto_now)] are only valid on FieldDate, FieldTime, or FieldDateTime",
            ));
        }
    }

    // Default value
    let default_val = if attr.auto_now_add || attr.auto_now {
        quote! { Some(#core::DefaultValue::CurrentTimestamp) }
    } else if let Some(d) = attr.default {
        quote! { Some(#core::DefaultValue::Literal(#d.to_string())) }
    } else {
        quote! { None }
    };

    Ok(quote! {
        #core::ColumnDef {
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

/// Generate the expression to read a field from a RangoRow.
fn field_type_to_getter(type_str: &str, col: &str, nullable: bool, core: &TokenStream) -> TokenStream {
    let getter = match type_str {
        "FieldBool"     => quote! { row.get_bool(#col).map(#core::FieldBool)? },
        "FieldSmallInt" => quote! { row.get_i16(#col).map(#core::FieldSmallInt)? },
        "FieldInt"      => quote! { row.get_i32(#col).map(#core::FieldInt)? },
        "FieldBigInt"   => quote! { row.get_i64(#col).map(#core::FieldBigInt)? },
        "FieldFloat"    => quote! { row.get_f32(#col).map(#core::FieldFloat)? },
        "FieldDouble"   => quote! { row.get_f64(#col).map(#core::FieldDouble)? },
        "FieldText"     => quote! { row.get_string(#col).map(#core::FieldText)? },
        "FieldEmail"    => quote! { row.get_string(#col).map(#core::FieldEmail)? },
        "FieldUrl"      => quote! { row.get_string(#col).map(#core::FieldUrl)? },
        "FieldBytes"    => quote! { row.get_bytes(#col).map(#core::FieldBytes)? },
        "FieldUuid"     => quote! { row.get_uuid(#col).map(#core::FieldUuid)? },
        "FieldDateTime" => quote! { row.get_datetime(#col).map(#core::FieldDateTime)? },
        "FieldDate"     => quote! { row.get_date(#col).map(#core::FieldDate)? },
        "FieldTime"     => quote! { row.get_time(#col).map(#core::FieldTime)? },
        "FieldJson"     => quote! { row.get_json(#col).map(#core::FieldJson)? },
        s if s.starts_with("ForeignKey<") => {
            // Extract the type parameter — not used at runtime, only for type safety
            quote! { row.get_uuid(#col).map(|id| #core::ForeignKey::new(id))? }
        }
        s if s.starts_with("FieldVarchar<") => {
            let (min, max) = parse_two_generics(s, "FieldVarchar");
            let min_lit = proc_macro2::Literal::usize_unsuffixed(min);
            let max_lit = proc_macro2::Literal::usize_unsuffixed(max);
            quote! { row.get_string(#col).map(|s| #core::FieldVarchar::<#min_lit, #max_lit>(s))? }
        }
        s if s.starts_with("FieldPassword<") => {
            let (min, max) = parse_two_generics(s, "FieldPassword");
            let min_lit = proc_macro2::Literal::usize_unsuffixed(min);
            let max_lit = proc_macro2::Literal::usize_unsuffixed(max);
            quote! { row.get_string(#col).map(|s| #core::FieldPassword::<#min_lit, #max_lit>(s))? }
        }
        s if s.starts_with("FieldRange<") => {
            let (min, max) = parse_two_generics_i64(s, "FieldRange");
            let min_lit = proc_macro2::Literal::i64_unsuffixed(min);
            let max_lit = proc_macro2::Literal::i64_unsuffixed(max);
            quote! { row.get_i64(#col).map(|v| #core::FieldRange::<#min_lit, #max_lit>(v))? }
        }
        _ => quote! { compile_error!("Unknown field type in FromRow") },
    };

    if nullable {
        match type_str {
            "FieldBool"     => quote! { if row.is_null(#col) { None } else { Some(row.get_bool(#col).map(#core::FieldBool)?) } },
            "FieldSmallInt" => quote! { if row.is_null(#col) { None } else { Some(row.get_i16(#col).map(#core::FieldSmallInt)?) } },
            "FieldInt"      => quote! { if row.is_null(#col) { None } else { Some(row.get_i32(#col).map(#core::FieldInt)?) } },
            "FieldBigInt"   => quote! { if row.is_null(#col) { None } else { Some(row.get_i64(#col).map(#core::FieldBigInt)?) } },
            "FieldText"     => quote! { if row.is_null(#col) { None } else { Some(row.get_string(#col).map(#core::FieldText)?) } },
            "FieldEmail"    => quote! { if row.is_null(#col) { None } else { Some(row.get_string(#col).map(#core::FieldEmail)?) } },
            "FieldUuid"     => quote! { if row.is_null(#col) { None } else { Some(row.get_uuid(#col).map(#core::FieldUuid)?) } },
            "FieldDateTime" => quote! { if row.is_null(#col) { None } else { Some(row.get_datetime(#col).map(#core::FieldDateTime)?) } },
            "FieldJson"     => quote! { if row.is_null(#col) { None } else { Some(row.get_json(#col).map(#core::FieldJson)?) } },
            s if s.starts_with("ForeignKey<") => {
                quote! { if row.is_null(#col) { None } else { Some(row.get_uuid(#col).map(|id| #core::ForeignKey::new(id))?) } }
            }
            s if s.starts_with("FieldVarchar<") => {
                let (min, max) = parse_two_generics(s, "FieldVarchar");
                let min_lit = proc_macro2::Literal::usize_unsuffixed(min);
                let max_lit = proc_macro2::Literal::usize_unsuffixed(max);
                quote! { if row.is_null(#col) { None } else { Some(row.get_string(#col).map(|s| #core::FieldVarchar::<#min_lit, #max_lit>(s))?) } }
            }
            _ => quote! { if row.is_null(#col) { None } else { Some(#getter) } },
        }
    } else {
        getter
    }
}

fn parse_two_generics(s: &str, prefix: &str) -> (usize, usize) {
    let inner = s.trim_start_matches(&format!("{}<", prefix)).trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 {
        let min = parts[0].trim().parse().unwrap_or(0);
        let max = parts[1].trim().parse().unwrap_or(255);
        (min, max)
    } else {
        (0, 255)
    }
}

fn parse_two_generics_i64(s: &str, prefix: &str) -> (i64, i64) {
    let inner = s.trim_start_matches(&format!("{}<", prefix)).trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 {
        let min = parts[0].trim().parse().unwrap_or(i64::MIN);
        let max = parts[1].trim().parse().unwrap_or(i64::MAX);
        (min, max)
    } else {
        (i64::MIN, i64::MAX)
    }
}

/// Returns true if the type is ManyToMany<T>.
fn is_many_to_many(ty: &Type) -> bool {
    let s = quote!(#ty).to_string().replace(" ", "");
    s.starts_with("ManyToMany<")
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
fn map_field_type(ty: &Type, core: &TokenStream) -> Result<TokenStream> {
    let type_str = quote!(#ty).to_string().replace(" ", "");

    let col_type = match type_str.as_str() {
        "FieldBool"       => quote! { #core::ColumnType::Bool },
        "FieldSmallInt"   => quote! { #core::ColumnType::SmallInt },
        "FieldInt"        => quote! { #core::ColumnType::Int },
        "FieldBigInt"     => quote! { #core::ColumnType::BigInt },
        "FieldFloat"      => quote! { #core::ColumnType::Float },
        "FieldDouble"     => quote! { #core::ColumnType::Double },
        "FieldText"       => quote! { #core::ColumnType::Text },
        "FieldEmail"      => quote! { #core::ColumnType::Varchar(254) },
        "FieldUrl"        => quote! { #core::ColumnType::Varchar(2048) },
        "FieldBytes"      => quote! { #core::ColumnType::Bytea },
        "FieldUuid"       => quote! { #core::ColumnType::Uuid },
        "FieldDate"       => quote! { #core::ColumnType::Date },
        "FieldTime"       => quote! { #core::ColumnType::Time },
        "FieldDateTime"   => quote! { #core::ColumnType::DateTime },
        "FieldJson"       => quote! { #core::ColumnType::Jsonb },
        s if s.starts_with("ForeignKey<")    => quote! { #core::ColumnType::Uuid },
        s if s.starts_with("FieldVarchar<")  => parse_varchar(s, core)?,
        s if s.starts_with("FieldDecimal<")  => parse_decimal(s, core)?,
        s if s.starts_with("FieldPassword<") => parse_password(s, core)?,
        s if s.starts_with("FieldRange<")    => quote! { #core::ColumnType::BigInt },
        _ => return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("Unknown Rango field type: `{}`. Use a FieldXxx type.", type_str),
        )),
    };
    Ok(col_type)
}

fn parse_varchar(s: &str, core: &TokenStream) -> Result<TokenStream> {
    let inner = s.trim_start_matches("FieldVarchar<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 {
        if let Ok(max) = parts[1].trim().parse::<u32>() {
            return Ok(quote! { #core::ColumnType::Varchar(#max) });
        }
    }
    Err(syn::Error::new(proc_macro2::Span::call_site(),
        format!("Invalid FieldVarchar syntax: `{}`", s)))
}

fn parse_decimal(s: &str, core: &TokenStream) -> Result<TokenStream> {
    let inner = s.trim_start_matches("FieldDecimal<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 {
        if let (Ok(p), Ok(sc)) = (parts[0].trim().parse::<u8>(), parts[1].trim().parse::<u8>()) {
            return Ok(quote! { #core::ColumnType::Decimal { precision: #p, scale: #sc } });
        }
    }
    Err(syn::Error::new(proc_macro2::Span::call_site(),
        format!("Invalid FieldDecimal syntax: `{}`", s)))
}

fn parse_password(s: &str, core: &TokenStream) -> Result<TokenStream> {
    let inner = s.trim_start_matches("FieldPassword<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 {
        if let Ok(max) = parts[1].trim().parse::<u32>() {
            return Ok(quote! { #core::ColumnType::Varchar(#max) });
        }
    }
    Err(syn::Error::new(proc_macro2::Span::call_site(),
        format!("Invalid FieldPassword syntax: `{}`", s)))
}
