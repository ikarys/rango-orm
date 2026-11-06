use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Data, DeriveInput, Field, Fields, Lit, Meta, Result, Token, Type, punctuated::Punctuated,
};

use crate::rango_core_path;
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
    on_delete: Option<String>,
}

/// Parsed `#[model(...)]` attributes on the struct.
#[derive(Default)]
struct ModelAttr {
    table: Option<String>,
    comment: Option<String>,
    managed: Option<bool>,
    ordering: Vec<syn::Expr>,
    constraints: Vec<syn::Expr>,
}

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let struct_name = &input.ident;
    let model_attr = parse_model_attr(&input)?;

    // Table name: explicit override or snake_case of struct name
    let table_name = model_attr
        .table
        .unwrap_or_else(|| to_snake_case(&struct_name.to_string()));

    let managed = model_attr.managed.unwrap_or(true);
    let ordering_exprs = &model_attr.ordering;
    let constraints_exprs = &model_attr.constraints;

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    struct_name,
                    "Model requires named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                struct_name,
                "Model can only be derived on structs",
            ));
        }
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
    let mut pk_value_expr = quote! { #core::SqlValue::NullUuid }; // placeholder — overwritten when PK field is found
    let mut pk_col_name = "id".to_string();
    let mut pk_found = false;
    let mut set_pk_body = quote! { let _ = id; };

    // ManyToMany fields are never read by generated code, so `dead_code` would flag them
    // in every model that declares one; this read silences it.
    let m2m_reads: Vec<_> = fields
        .iter()
        .filter(|f| is_many_to_many(&f.ty))
        .map(|f| {
            let fname = f.ident.as_ref().unwrap();
            quote! { let _ = &self.#fname; }
        })
        .collect();

    for f in fields.iter() {
        if is_many_to_many(&f.ty) {
            continue;
        }
        let fname = f.ident.as_ref().unwrap();
        let attr = parse_field_attr(f)?;
        let is_pk = fname == "id" || attr.primary_key;
        let col_name = column_name(fname, attr.column);

        if is_pk {
            pk_found = true;
            pk_col_name = col_name.clone();
            pk_value_expr = quote! {
                #core::ToSqlValue::to_sql_value(&self.#fname)
            };
            let ty = &f.ty;
            let ty_str = quote!(#ty).to_string().replace(' ', "");
            // Path-qualified types too (`#[rango::model]` injects `rango_core::FieldAutoBigInt`).
            match ty_str.rsplit("::").next().unwrap_or("") {
                "FieldAutoInt" => {
                    set_pk_body =
                        quote! { self.#fname = #core::FieldAutoInt(i32::try_from(id).ok()); };
                }
                "FieldAutoBigInt" => {
                    set_pk_body = quote! { self.#fname = #core::FieldAutoBigInt(Some(id)); };
                }
                _ => {}
            }
        } else {
            field_value_entries.push(quote! {
                (#col_name, #core::ToSqlValue::to_sql_value(&self.#fname))
            });
        }
    }

    if !pk_found {
        return Err(syn::Error::new_spanned(
            struct_name,
            "Rango: no primary key found. Declare a field named `id` or annotate one with `#[field(primary_key)]`, or use `#[rango::model]` to add an implicit `id`",
        ));
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
        let col_name = column_name(fname, parse_field_attr(f)?.column);
        let (nullable, inner_ty) = extract_option(&f.ty);
        let type_str = type_name(inner_ty);

        let getter = field_type_to_getter(&type_str, &col_name, nullable, &core);
        from_row_fields.push(quote! { #fname: #getter });
    }

    // Typed column constants: `User::AGE` → `Column<User, FieldInt>`; filters check
    // the column and the value type at compile time.
    let mut column_consts = Vec::new();
    for f in fields.iter().filter(|f| !is_many_to_many(&f.ty)) {
        let fname = f.ident.as_ref().unwrap();
        let col_name = column_name(fname, parse_field_attr(f)?.column);
        let const_name = syn::Ident::new(
            &fname.to_string().trim_start_matches("r#").to_uppercase(),
            fname.span(),
        );
        let (_, field_ty) = extract_option(&f.ty);
        let doc = format!("Typed reference to the `{col_name}` column.");
        column_consts.push(quote! {
            #[doc = #doc]
            pub const #const_name: #core::Column<Self, #field_ty> = #core::Column::new(#col_name);
        });
    }
    // `Article::TAGS` for `tags: ManyToMany<Tag>`, used by `prefetch_related`.
    for f in fields.iter().filter(|f| is_many_to_many(&f.ty)) {
        let fname = f.ident.as_ref().unwrap();
        let const_name = syn::Ident::new(
            &fname.to_string().trim_start_matches("r#").to_uppercase(),
            fname.span(),
        );
        let Some(to_ty) = first_generic(&f.ty) else {
            return Err(syn::Error::new_spanned(&f.ty, "expected ManyToMany<Model>"));
        };
        let doc = format!("Typed reference to the `{fname}` many-to-many relation.");
        column_consts.push(quote! {
            #[doc = #doc]
            pub const #const_name: #core::ManyToManyField<Self, #to_ty> = #core::ManyToManyField::new();
        });
    }

    let comment_expr = match &model_attr.comment {
        Some(c) => quote! { Some(#c.to_string()) },
        None => quote! { None },
    };

    let core2 = rango_core_path();
    Ok(quote! {
        impl #core2::Model for #struct_name {
            fn table_name() -> &'static str {
                #table_name
            }

            fn schema() -> #core2::TableSchema {
                use #core2::{CheckConstraint, UniqueConstraint, asc, desc};
                #core2::TableSchema {
                    table_name: #table_name.to_string(),
                    columns: vec![
                        #(#column_defs),*
                    ],
                    constraints: vec![
                        #((#constraints_exprs).into()),*
                    ],
                    ordering: vec![
                        #(#ordering_exprs),*
                    ],
                    managed: #managed,
                    comment: #comment_expr,
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

        #[allow(dead_code)]
        impl #struct_name {
            #(#column_consts)*
        }

        impl #core2::ModelValues for #struct_name {
            fn field_values(&self) -> Vec<(&'static str, #core2::SqlValue)> {
                #(#m2m_reads)*
                vec![#(#field_value_entries),*]
            }

            fn pk_value(&self) -> #core2::SqlValue {
                #pk_value_expr
            }

            fn pk_column() -> &'static str {
                #pk_col_name
            }

            fn set_generated_pk(&mut self, id: i64) {
                #set_pk_body
            }
        }
    })
}

/// DB column name: `#[field(column = "...")]` override, else the field name (without `r#`).
fn column_name(field: &syn::Ident, column: Option<String>) -> String {
    column.unwrap_or_else(|| field.to_string().trim_start_matches("r#").to_string())
}

/// Parse `#[model(table = "...", comment = "...")]`
fn parse_model_attr(input: &DeriveInput) -> Result<ModelAttr> {
    let mut attr = ModelAttr::default();
    for a in &input.attrs {
        if !a.path().is_ident("model") {
            continue;
        }
        let nested = a.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
        for meta in nested {
            match meta {
                Meta::NameValue(nv) if nv.path.is_ident("table") => {
                    if let syn::Expr::Lit(expr_lit) = &nv.value
                        && let Lit::Str(s) = &expr_lit.lit
                    {
                        attr.table = Some(s.value());
                    }
                }
                Meta::NameValue(nv) if nv.path.is_ident("comment") => {
                    if let syn::Expr::Lit(expr_lit) = &nv.value
                        && let Lit::Str(s) = &expr_lit.lit
                    {
                        attr.comment = Some(s.value());
                    }
                }
                Meta::NameValue(nv) if nv.path.is_ident("managed") => {
                    if let syn::Expr::Lit(expr_lit) = &nv.value
                        && let Lit::Bool(b) = &expr_lit.lit
                    {
                        attr.managed = Some(b.value);
                    }
                }
                Meta::NameValue(nv) if nv.path.is_ident("ordering") => {
                    if let syn::Expr::Array(arr) = &nv.value {
                        attr.ordering = arr.elems.iter().cloned().collect();
                    }
                }
                Meta::NameValue(nv) if nv.path.is_ident("constraints") => {
                    if let syn::Expr::Array(arr) = &nv.value {
                        attr.constraints = arr.elems.iter().cloned().collect();
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
        let nested = a.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
        for meta in nested {
            match &meta {
                Meta::Path(p) if p.is_ident("unique") => attr.unique = true,
                Meta::Path(p) if p.is_ident("index") => attr.index = true,
                Meta::Path(p) if p.is_ident("primary_key") => attr.primary_key = true,
                Meta::Path(p) if p.is_ident("auto_now_add") => attr.auto_now_add = true,
                Meta::Path(p) if p.is_ident("auto_now") => attr.auto_now = true,
                Meta::NameValue(nv) if nv.path.is_ident("column") => {
                    if let syn::Expr::Lit(expr_lit) = &nv.value
                        && let Lit::Str(s) = &expr_lit.lit
                    {
                        attr.column = Some(s.value());
                    }
                }
                Meta::NameValue(nv) if nv.path.is_ident("default") => {
                    if let syn::Expr::Lit(expr_lit) = &nv.value
                        && let Lit::Str(s) = &expr_lit.lit
                    {
                        attr.default = Some(s.value());
                    }
                }
                Meta::NameValue(nv) if nv.path.is_ident("on_delete") => {
                    if let syn::Expr::Lit(expr_lit) = &nv.value
                        && let Lit::Str(s) = &expr_lit.lit
                    {
                        attr.on_delete = Some(s.value());
                    }
                }
                Meta::NameValue(nv) if nv.path.is_ident("comment") => {
                    if let syn::Expr::Lit(expr_lit) = &nv.value
                        && let Lit::Str(s) = &expr_lit.lit
                    {
                        attr.comment = Some(s.value());
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
    let col_name = column_name(field_name, attr.column);

    // Detect nullable: if the type is Option<T>
    let (nullable, inner_ty) = extract_option(&field.ty);

    // Auto-detect primary key: field named "id" gets PK by default
    let is_pk = attr.primary_key || field_name == "id";
    let is_index = attr.index;
    let is_unique = is_pk || attr.unique; // PK implies unique

    // Map Rust field type to ColumnType
    let col_type = map_field_type(inner_ty, core)?;
    let is_foreign_key = type_name(inner_ty).starts_with("ForeignKey<");

    // Validate: auto_now_add / auto_now only on date/time fields
    if attr.auto_now_add || attr.auto_now {
        let type_str = type_name(inner_ty);
        if !matches!(
            type_str.as_str(),
            "FieldDate" | "FieldTime" | "FieldDateTime"
        ) {
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

    // Same rules and defaults as `rango makemigrations`, so the compiled schema and the
    // migrated one cannot drift (see rango-orm-cli/tests/contract.rs).
    let references = if is_foreign_key {
        let target = first_generic(inner_ty)
            .ok_or_else(|| syn::Error::new_spanned(&field.ty, "expected ForeignKey<Model>"))?;
        let action = match attr.on_delete.as_deref() {
            None | Some("cascade") => quote! { Cascade },
            Some("set_null") => {
                if !nullable {
                    return Err(syn::Error::new_spanned(
                        field,
                        "on_delete = \"set_null\" requires an Option<ForeignKey<...>> field",
                    ));
                }
                quote! { SetNull }
            }
            Some("set_default") => quote! { SetDefault },
            Some("restrict") => quote! { Restrict },
            Some("no_action") => quote! { NoAction },
            Some(other) => {
                return Err(syn::Error::new_spanned(
                    field,
                    format!("Invalid on_delete value: {other}"),
                ));
            }
        };
        quote! {
            Some(#core::FkReference {
                table: <#target as #core::Model>::table_name().to_string(),
                column: <#target as #core::ModelValues>::pk_column().to_string(),
                on_delete: #core::ReferentialAction::#action,
                on_update: #core::ReferentialAction::NoAction,
            })
        }
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
            references: #references,
            index: #is_index || #is_foreign_key,
        }
    })
}

/// Generate the expression to read a field from a RangoRow.
pub(crate) fn field_type_to_getter(
    type_str: &str,
    col: &impl quote::ToTokens,
    nullable: bool,
    core: &TokenStream,
) -> TokenStream {
    let getter = match type_str {
        "FieldBool" => quote! { row.get_bool(#col).map(#core::FieldBool)? },
        "FieldSmallInt" => quote! { row.get_i16(#col).map(#core::FieldSmallInt)? },
        "FieldInt" => quote! { row.get_i32(#col).map(#core::FieldInt)? },
        "FieldBigInt" => quote! { row.get_i64(#col).map(#core::FieldBigInt)? },
        "FieldAutoInt" => quote! { #core::FieldAutoInt(Some(row.get_i32(#col)?)) },
        "FieldAutoBigInt" => quote! { #core::FieldAutoBigInt(Some(row.get_i64(#col)?)) },
        "FieldFloat" => quote! { row.get_f32(#col).map(#core::FieldFloat)? },
        "FieldDouble" => quote! { row.get_f64(#col).map(#core::FieldDouble)? },
        "FieldText" => quote! { row.get_string(#col).map(#core::FieldText)? },
        "FieldEmail" => quote! { row.get_string(#col).map(#core::FieldEmail)? },
        "FieldUrl" => quote! { row.get_string(#col).map(#core::FieldUrl)? },
        "FieldBytes" => quote! { row.get_bytes(#col).map(#core::FieldBytes)? },
        "FieldUuid" => quote! { row.get_uuid(#col).map(#core::FieldUuid)? },
        "FieldDateTime" => quote! { row.get_datetime(#col).map(#core::FieldDateTime)? },
        "FieldDate" => quote! { row.get_date(#col).map(#core::FieldDate)? },
        "FieldTime" => quote! { row.get_time(#col).map(#core::FieldTime)? },
        "FieldJson" => quote! { row.get_json(#col).map(#core::FieldJson)? },
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
            quote! { row.get_string(#col).map(|s| #core::FieldPassword::<#min_lit, #max_lit>::from_hash(s))? }
        }
        s if s.starts_with("FieldDecimal<") => {
            let (p, sc) = parse_two_generics(s, "FieldDecimal");
            let p_lit = proc_macro2::Literal::u8_unsuffixed(p as u8);
            let sc_lit = proc_macro2::Literal::u8_unsuffixed(sc as u8);
            quote! { row.get_decimal(#col).map(#core::FieldDecimal::<#p_lit, #sc_lit>::from_decimal)? }
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
            "FieldBool" => {
                quote! { if row.is_null(#col) { None } else { Some(row.get_bool(#col).map(#core::FieldBool)?) } }
            }
            "FieldSmallInt" => {
                quote! { if row.is_null(#col) { None } else { Some(row.get_i16(#col).map(#core::FieldSmallInt)?) } }
            }
            "FieldInt" => {
                quote! { if row.is_null(#col) { None } else { Some(row.get_i32(#col).map(#core::FieldInt)?) } }
            }
            "FieldBigInt" => {
                quote! { if row.is_null(#col) { None } else { Some(row.get_i64(#col).map(#core::FieldBigInt)?) } }
            }
            "FieldText" => {
                quote! { if row.is_null(#col) { None } else { Some(row.get_string(#col).map(#core::FieldText)?) } }
            }
            "FieldEmail" => {
                quote! { if row.is_null(#col) { None } else { Some(row.get_string(#col).map(#core::FieldEmail)?) } }
            }
            "FieldUuid" => {
                quote! { if row.is_null(#col) { None } else { Some(row.get_uuid(#col).map(#core::FieldUuid)?) } }
            }
            "FieldDateTime" => {
                quote! { if row.is_null(#col) { None } else { Some(row.get_datetime(#col).map(#core::FieldDateTime)?) } }
            }
            "FieldJson" => {
                quote! { if row.is_null(#col) { None } else { Some(row.get_json(#col).map(#core::FieldJson)?) } }
            }
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
    let inner = s
        .trim_start_matches(&format!("{}<", prefix))
        .trim_end_matches('>');
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
    let inner = s
        .trim_start_matches(&format!("{}<", prefix))
        .trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 {
        let min = parts[0].trim().parse().unwrap_or(i64::MIN);
        let max = parts[1].trim().parse().unwrap_or(i64::MAX);
        (min, max)
    } else {
        (i64::MIN, i64::MAX)
    }
}

/// The first generic argument of a path type: `T` in `ManyToMany<T>`.
fn first_generic(ty: &Type) -> Option<&Type> {
    let Type::Path(p) = ty else { return None };
    let syn::PathArguments::AngleBracketed(args) = &p.path.segments.last()?.arguments else {
        return None;
    };
    args.args.iter().find_map(|a| match a {
        syn::GenericArgument::Type(t) => Some(t),
        _ => None,
    })
}

/// Type text without its module path: `rango::FieldVarchar<1, 100>` → `FieldVarchar<1,100>`,
/// so field types match whether they are imported or written with a path.
pub(crate) fn type_name(ty: &Type) -> String {
    let last = match ty {
        Type::Path(tp) => tp.path.segments.last().map(|seg| quote!(#seg)),
        _ => None,
    };
    last.unwrap_or_else(|| quote!(#ty))
        .to_string()
        .replace(' ', "")
}

/// Returns true if the type is ManyToMany<T>.
fn is_many_to_many(ty: &Type) -> bool {
    let s = type_name(ty);
    s.starts_with("ManyToMany<")
}

/// Returns (is_nullable, inner_type).
/// `Option<T>` → (true, T), anything else → (false, original_type)
pub(crate) fn extract_option(ty: &Type) -> (bool, &Type) {
    if let Type::Path(tp) = ty
        && let Some(seg) = tp.path.segments.last()
        && seg.ident == "Option"
        && let syn::PathArguments::AngleBracketed(args) = &seg.arguments
        && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
    {
        return (true, inner);
    }
    (false, ty)
}

/// Map a Rango field type path to a `ColumnType` token.
fn map_field_type(ty: &Type, core: &TokenStream) -> Result<TokenStream> {
    let type_str = type_name(ty);

    let col_type = match type_str.as_str() {
        "FieldBool" => quote! { #core::ColumnType::Bool },
        "FieldSmallInt" => quote! { #core::ColumnType::SmallInt },
        "FieldInt" => quote! { #core::ColumnType::Int },
        "FieldBigInt" => quote! { #core::ColumnType::BigInt },
        "FieldAutoInt" => quote! { #core::ColumnType::AutoInt },
        "FieldAutoBigInt" => quote! { #core::ColumnType::AutoBigInt },
        "FieldFloat" => quote! { #core::ColumnType::Float },
        "FieldDouble" => quote! { #core::ColumnType::Double },
        "FieldText" => quote! { #core::ColumnType::Text },
        "FieldEmail" => quote! { #core::ColumnType::Varchar(254) },
        "FieldUrl" => quote! { #core::ColumnType::Varchar(2048) },
        "FieldBytes" => quote! { #core::ColumnType::Bytea },
        "FieldUuid" => quote! { #core::ColumnType::Uuid },
        "FieldDate" => quote! { #core::ColumnType::Date },
        "FieldTime" => quote! { #core::ColumnType::Time },
        "FieldDateTime" => quote! { #core::ColumnType::DateTime },
        "FieldJson" => quote! { #core::ColumnType::Jsonb },
        s if s.starts_with("ForeignKey<") => quote! { #core::ColumnType::Uuid },
        s if s.starts_with("FieldVarchar<") => parse_varchar(s, core)?,
        s if s.starts_with("FieldDecimal<") => parse_decimal(s, core)?,
        s if s.starts_with("FieldPassword<") => parse_password(s, core)?,
        s if s.starts_with("FieldRange<") => quote! { #core::ColumnType::BigInt },
        _ => {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                format!(
                    "Unknown Rango field type: `{}`. Use a FieldXxx type.",
                    type_str
                ),
            ));
        }
    };
    Ok(col_type)
}

fn parse_varchar(s: &str, core: &TokenStream) -> Result<TokenStream> {
    let inner = s.trim_start_matches("FieldVarchar<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2
        && let Ok(max) = parts[1].trim().parse::<u32>()
    {
        return Ok(quote! { #core::ColumnType::Varchar(#max) });
    }
    Err(syn::Error::new(
        proc_macro2::Span::call_site(),
        format!("Invalid FieldVarchar syntax: `{}`", s),
    ))
}

fn parse_decimal(s: &str, core: &TokenStream) -> Result<TokenStream> {
    let inner = s.trim_start_matches("FieldDecimal<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2
        && let (Ok(p), Ok(sc)) = (parts[0].trim().parse::<u8>(), parts[1].trim().parse::<u8>())
    {
        return Ok(quote! { #core::ColumnType::Decimal { precision: #p, scale: #sc } });
    }
    Err(syn::Error::new(
        proc_macro2::Span::call_site(),
        format!("Invalid FieldDecimal syntax: `{}`", s),
    ))
}

fn parse_password(s: &str, core: &TokenStream) -> Result<TokenStream> {
    let inner = s.trim_start_matches("FieldPassword<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 && parts[1].trim().parse::<u32>().is_ok() {
        return Ok(quote! { #core::ColumnType::Text });
    }
    Err(syn::Error::new(
        proc_macro2::Span::call_site(),
        format!("Invalid FieldPassword syntax: `{}`", s),
    ))
}
