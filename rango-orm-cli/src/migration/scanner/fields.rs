//! Struct field to `ColumnDef`: `#[field(...)]` attributes and the field type.

use super::*;

/// Build a ColumnDef from a struct field
pub(super) fn build_column_def(field: &syn::Field) -> Result<ColumnDef> {
    let field_name = field
        .ident
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("unnamed field"))?
        .to_string();

    let field_attrs = parse_field_attrs(field);

    let col_name = field_attrs.column.unwrap_or_else(|| field_name.clone());
    let is_pk = field_attrs.primary_key || field_name == "id";

    let (nullable, inner_ty) = extract_option(&field.ty);
    let type_str = type_name(inner_ty);
    let is_foreign_key = type_str.starts_with("ForeignKey<");
    let col_type = map_field_type(inner_ty)?;

    let on_delete = parse_on_delete(&field_attrs.on_delete)?;
    if on_delete == ReferentialAction::SetNull && !nullable {
        bail!("on_delete = \"set_null\" requires an Option<ForeignKey<...>> field");
    }

    let references = if is_foreign_key {
        let table = extract_foreign_key_target(inner_ty).unwrap_or_default();
        Some(FkReference {
            table,
            column: "id".to_string(),
            on_delete,
            on_update: ReferentialAction::NoAction,
        })
    } else {
        None
    };

    let default = if field_attrs.auto_now_add || field_attrs.auto_now {
        Some(DefaultValue::CurrentTimestamp)
    } else {
        field_attrs.default.map(DefaultValue::Literal)
    };

    Ok(ColumnDef {
        name: col_name,
        col_type,
        nullable,
        primary_key: is_pk,
        unique: is_pk || field_attrs.unique,
        default,
        references,
        index: field_attrs.index || is_foreign_key,
    })
}

#[derive(Default)]
pub(super) struct FieldAttrs {
    unique: bool,
    index: bool,
    pub(super) primary_key: bool,
    auto_now_add: bool,
    auto_now: bool,
    column: Option<String>,
    default: Option<String>,
    on_delete: Option<String>,
}

pub(super) fn parse_field_attrs(field: &syn::Field) -> FieldAttrs {
    let mut attrs = FieldAttrs::default();
    for attr in &field.attrs {
        if !attr.path().is_ident("field") {
            continue;
        }
        if let Ok(list) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) {
            for meta in list {
                match &meta {
                    syn::Meta::Path(p) if p.is_ident("unique") => attrs.unique = true,
                    syn::Meta::Path(p) if p.is_ident("index") => attrs.index = true,
                    syn::Meta::Path(p) if p.is_ident("primary_key") => attrs.primary_key = true,
                    syn::Meta::Path(p) if p.is_ident("auto_now_add") => attrs.auto_now_add = true,
                    syn::Meta::Path(p) if p.is_ident("auto_now") => attrs.auto_now = true,
                    syn::Meta::NameValue(nv) if nv.path.is_ident("column") => {
                        if let syn::Expr::Lit(e) = &nv.value
                            && let syn::Lit::Str(s) = &e.lit
                        {
                            attrs.column = Some(s.value());
                        }
                    }
                    syn::Meta::NameValue(nv) if nv.path.is_ident("default") => {
                        if let syn::Expr::Lit(e) = &nv.value
                            && let syn::Lit::Str(s) = &e.lit
                        {
                            attrs.default = Some(s.value());
                        }
                    }
                    syn::Meta::NameValue(nv) if nv.path.is_ident("on_delete") => {
                        if let syn::Expr::Lit(e) = &nv.value
                            && let syn::Lit::Str(s) = &e.lit
                        {
                            attrs.on_delete = Some(s.value());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    attrs
}

/// Type text without its module path (`rango::FieldVarchar<1, 100>` becomes `FieldVarchar<1,100>`),
/// the same normalisation as the derive, so imported and path-qualified types mean the same.
pub(super) fn type_name(ty: &Type) -> String {
    let last = match ty {
        Type::Path(tp) => tp.path.segments.last().map(|seg| quote::quote!(#seg)),
        _ => None,
    };
    last.unwrap_or_else(|| quote::quote!(#ty))
        .to_string()
        .replace(' ', "")
}

pub(super) fn extract_option(ty: &Type) -> (bool, &Type) {
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

pub(super) fn map_field_type(ty: &Type) -> Result<ColumnType> {
    let s = type_name(ty);
    if s.starts_with("ForeignKey<") {
        // Resolved against the target primary key later (`resolve_model_tables`).
        return Ok(ColumnType::Uuid);
    }
    type_map::column_type(&s).map_err(|e| match e {
        TypeMapError::Unknown => {
            anyhow::anyhow!("Unknown field type: {s}: {}", type_map::rango_type_hint(&s))
        }
        TypeMapError::Invalid(name) => anyhow::anyhow!("Invalid {name}: {s}"),
    })
}

pub(super) fn parse_on_delete(value: &Option<String>) -> Result<ReferentialAction> {
    match value.as_deref() {
        None | Some("cascade") => Ok(ReferentialAction::Cascade),
        Some("set_null") => Ok(ReferentialAction::SetNull),
        Some("set_default") => Ok(ReferentialAction::SetDefault),
        Some("restrict") => Ok(ReferentialAction::Restrict),
        Some("no_action") => Ok(ReferentialAction::NoAction),
        Some(other) => bail!("Invalid on_delete value: {}", other),
    }
}

pub(super) fn extract_foreign_key_target(ty: &Type) -> Option<String> {
    let s = type_name(ty);
    let inner = s.strip_prefix("ForeignKey<")?.strip_suffix('>')?;
    let last = inner.rsplit("::").next()?;
    Some(last.to_string())
}
