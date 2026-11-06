use anyhow::{Context, Result, bail};
use rango_orm_core::{
    CheckConstraint, ColumnDef, ColumnType, Constraint, DefaultValue, FkReference, OrderBy,
    OrderDir, ReferentialAction, TableSchema, UniqueConstraint,
};
use std::collections::HashMap;
use std::path::Path;
use syn::{File, ItemStruct, Type, visit::Visit};
use walkdir::WalkDir;

/// Scan a source directory and return (schemas, m2m_relations).
pub fn scan_models(src_dir: &str) -> Result<(Vec<TableSchema>, Vec<M2MRelation>)> {
    let mut schemas = Vec::new();
    let mut m2m_relations = Vec::new();
    let mut model_tables: HashMap<String, String> = HashMap::new();
    let mut errors: Vec<String> = Vec::new();

    for entry in WalkDir::new(src_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "rs").unwrap_or(false))
    {
        let path = entry.path();
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        if !uses_rango(&content) {
            continue;
        }

        let file: File = syn::parse_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;

        let mut visitor = ModelVisitor::new(path);
        visitor.visit_file(&file);
        schemas.extend(visitor.schemas);
        m2m_relations.extend(visitor.m2m_relations);
        model_tables.extend(visitor.model_tables);
        errors.extend(visitor.errors);
    }

    if !errors.is_empty() {
        bail!("Field errors:\n{}", errors.join("\n"));
    }

    resolve_model_tables(&mut schemas, &mut m2m_relations, &model_tables);

    Ok((schemas, m2m_relations))
}

/// Quick check: does this file import rango?
fn uses_rango(content: &str) -> bool {
    content.contains("rango_orm_derive")
        || content.contains("rango_orm_core")
        || content.contains("use rango")
        || content.contains("rango::model")
}

/// A detected M2M relation between two table names.
#[derive(Debug, Clone)]
pub struct M2MRelation {
    pub from_table: String,
    pub to_table: String,
}

struct ModelVisitor<'a> {
    path: &'a Path,
    pub schemas: Vec<TableSchema>,
    pub m2m_relations: Vec<M2MRelation>,
    pub model_tables: HashMap<String, String>,
    pub errors: Vec<String>,
}

impl<'a> ModelVisitor<'a> {
    fn new(path: &'a Path) -> Self {
        Self {
            path,
            schemas: Vec::new(),
            m2m_relations: Vec::new(),
            model_tables: HashMap::new(),
            errors: Vec::new(),
        }
    }
}

impl<'ast> Visit<'ast> for ModelVisitor<'_> {
    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        let attr_pk = attr_macro_pk(node);
        if !has_derive_model(node) && attr_pk.is_none() {
            return;
        }

        let table_name = extract_table_name(node);
        self.model_tables
            .insert(node.ident.to_string(), table_name.clone());
        let mut columns = Vec::new();

        if let syn::Fields::Named(fields) = &node.fields {
            // `#[rango::model]` injects an implicit `id` first, as the macro does.
            if let Some(pk) = &attr_pk
                && !fields.named.iter().any(|f| {
                    f.ident.as_ref().is_some_and(|i| i == "id") || parse_field_attrs(f).primary_key
                })
            {
                let ty = match pk.as_str() {
                    "bigint" => "FieldAutoBigInt",
                    "int" => "FieldAutoInt",
                    _ => "FieldUuid",
                };
                let id: syn::Field =
                    syn::parse::Parser::parse_str(syn::Field::parse_named, &format!("id: {ty}"))
                        .expect("valid field");
                match build_column_def(&id) {
                    Ok(col) => columns.push(col),
                    Err(e) => self.errors.push(format!("{}: {}", node.ident, e)),
                }
            }
            for field in &fields.named {
                // Detect ManyToMany<T> — record relation, skip column
                let ty = &field.ty;
                let type_str = quote::quote!(#ty).to_string().replace(" ", "");
                if type_str.starts_with("ManyToMany<") {
                    let to_model = type_str
                        .trim_start_matches("ManyToMany<")
                        .trim_end_matches('>')
                        .to_string();
                    let to_ident = to_model.rsplit("::").next().unwrap_or(&to_model);
                    // Model ident here; `resolve_model_tables` maps it to the real table.
                    let to_table = to_ident.to_string();
                    self.m2m_relations.push(M2MRelation {
                        from_table: table_name.clone(),
                        to_table,
                    });
                    continue;
                }

                match build_column_def(field) {
                    Ok(col) => columns.push(col),
                    Err(e) => {
                        self.errors
                            .push(format!("{}: {}: {}", self.path.display(), node.ident, e))
                    }
                }
            }
        }

        let managed = extract_managed(node);
        if !managed {
            // Still track unmanaged models but mark them — caller filters them out
        }
        let ordering = extract_ordering(node);
        let constraints = extract_constraints(node);

        self.schemas.push(TableSchema {
            table_name,
            columns,
            constraints,
            ordering,
            managed,
            comment: None,
        });
    }
}

/// `#[rango::model]` / `#[model]` attribute macro: returns its `pk` argument
/// (default "uuid"). A bare `#[model(...)]` next to `derive(Model)` is the derive helper, not the macro.
fn attr_macro_pk(node: &ItemStruct) -> Option<String> {
    let derived = has_derive_model(node);
    node.attrs.iter().find_map(|attr| {
        let p = attr.path();
        let is_macro = p.segments.len() == 2
            && p.segments[0].ident == "rango"
            && p.segments[1].ident == "model"
            || (p.is_ident("model") && !derived);
        if !is_macro {
            return None;
        }
        let mut pk = "uuid".to_string();
        if let Ok(list) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) {
            for meta in list {
                if let syn::Meta::NameValue(nv) = meta
                    && nv.path.is_ident("pk")
                    && let syn::Expr::Lit(e) = &nv.value
                    && let syn::Lit::Str(s) = &e.lit
                {
                    pk = s.value();
                }
            }
        }
        Some(pk)
    })
}

/// Check if a struct has `#[derive(Model)]`
fn has_derive_model(node: &ItemStruct) -> bool {
    for attr in &node.attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }
        if let Ok(list) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
        ) {
            for path in list {
                if path.is_ident("Model") {
                    return true;
                }
            }
        }
    }
    false
}

/// Extract table name from #[model(table = "...")] or snake_case of struct name
fn extract_table_name(node: &ItemStruct) -> String {
    for attr in &node.attrs {
        if !attr.path().is_ident("model") {
            continue;
        }
        if let Ok(list) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) {
            for meta in list {
                if let syn::Meta::NameValue(nv) = meta
                    && nv.path.is_ident("table")
                    && let syn::Expr::Lit(expr_lit) = &nv.value
                    && let syn::Lit::Str(s) = &expr_lit.lit
                {
                    return s.value();
                }
            }
        }
    }
    to_snake_case(&node.ident.to_string())
}

/// Build a ColumnDef from a struct field
fn build_column_def(field: &syn::Field) -> Result<ColumnDef> {
    let field_name = field
        .ident
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("unnamed field"))?
        .to_string();

    let field_attrs = parse_field_attrs(field);

    let col_name = field_attrs.column.unwrap_or_else(|| field_name.clone());
    let is_pk = field_attrs.primary_key || field_name == "id";

    let (nullable, inner_ty) = extract_option(&field.ty);
    let type_str = quote::quote!(#inner_ty).to_string().replace(" ", "");
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
struct FieldAttrs {
    unique: bool,
    index: bool,
    primary_key: bool,
    auto_now_add: bool,
    auto_now: bool,
    column: Option<String>,
    default: Option<String>,
    on_delete: Option<String>,
}

fn parse_field_attrs(field: &syn::Field) -> FieldAttrs {
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

fn extract_option(ty: &Type) -> (bool, &Type) {
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

fn map_field_type(ty: &Type) -> Result<ColumnType> {
    let s = quote::quote!(#ty).to_string().replace(" ", "");
    let col = match s.as_str() {
        "FieldBool" => ColumnType::Bool,
        "FieldSmallInt" => ColumnType::SmallInt,
        "FieldInt" => ColumnType::Int,
        "FieldBigInt" => ColumnType::BigInt,
        "FieldAutoInt" => ColumnType::AutoInt,
        "FieldAutoBigInt" => ColumnType::AutoBigInt,
        "FieldFloat" => ColumnType::Float,
        "FieldDouble" => ColumnType::Double,
        "FieldText" => ColumnType::Text,
        "FieldEmail" => ColumnType::Varchar(254),
        "FieldUrl" => ColumnType::Varchar(2048),
        "FieldBytes" => ColumnType::Bytea,
        "FieldUuid" => ColumnType::Uuid,
        "FieldDate" => ColumnType::Date,
        "FieldTime" => ColumnType::Time,
        "FieldDateTime" => ColumnType::DateTime,
        "FieldJson" => ColumnType::Jsonb,
        s if s.starts_with("FieldVarchar<") => parse_varchar(s)?,
        s if s.starts_with("FieldPassword<") => parse_password(s)?,
        s if s.starts_with("FieldDecimal<") => parse_decimal(s)?,
        s if s.starts_with("FieldRange<") => ColumnType::BigInt,
        s if s.starts_with("ForeignKey<") => ColumnType::Uuid,
        _ => anyhow::bail!("Unknown field type: {}", s),
    };
    Ok(col)
}

fn parse_varchar(s: &str) -> Result<ColumnType> {
    let inner = s.trim_start_matches("FieldVarchar<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2
        && let Ok(max) = parts[1].trim().parse::<u32>()
    {
        return Ok(ColumnType::Varchar(max));
    }
    anyhow::bail!("Invalid FieldVarchar: {}", s)
}

fn parse_password(s: &str) -> Result<ColumnType> {
    let inner = s.trim_start_matches("FieldPassword<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 && parts[1].trim().parse::<u32>().is_ok() {
        return Ok(ColumnType::Text);
    }
    anyhow::bail!("Invalid FieldPassword: {}", s)
}

fn parse_decimal(s: &str) -> Result<ColumnType> {
    let inner = s.trim_start_matches("FieldDecimal<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2
        && let (Ok(p), Ok(sc)) = (parts[0].trim().parse::<u8>(), parts[1].trim().parse::<u8>())
    {
        return Ok(ColumnType::Decimal {
            precision: p,
            scale: sc,
        });
    }
    anyhow::bail!("Invalid FieldDecimal: {}", s)
}

/// Extract `managed = false` from `#[model(...)]`, defaults to `true`.
fn extract_managed(node: &ItemStruct) -> bool {
    for attr in &node.attrs {
        if !attr.path().is_ident("model") {
            continue;
        }
        if let Ok(list) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) {
            for meta in list {
                if let syn::Meta::NameValue(nv) = meta
                    && nv.path.is_ident("managed")
                    && let syn::Expr::Lit(expr_lit) = &nv.value
                    && let syn::Lit::Bool(b) = &expr_lit.lit
                {
                    return b.value;
                }
            }
        }
    }
    true
}

/// Extract `ordering = [asc("col"), desc("other")]` from `#[model(...)]`.
fn extract_ordering(node: &ItemStruct) -> Vec<OrderBy> {
    let mut result = Vec::new();
    for attr in &node.attrs {
        if !attr.path().is_ident("model") {
            continue;
        }
        if let Ok(list) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) {
            for meta in list {
                if let syn::Meta::NameValue(nv) = meta
                    && nv.path.is_ident("ordering")
                    && let syn::Expr::Array(arr) = &nv.value
                {
                    for elem in &arr.elems {
                        if let Some(ob) = parse_order_expr(elem) {
                            result.push(ob);
                        }
                    }
                }
            }
        }
    }
    result
}

/// Parse `asc("col")` or `desc("col")` into an `OrderBy`.
fn parse_order_expr(expr: &syn::Expr) -> Option<OrderBy> {
    if let syn::Expr::Call(call) = expr {
        let func = &call.func;
        let func_str = quote::quote!(#func).to_string().replace(" ", "");
        let args: Vec<syn::Expr> = call.args.iter().cloned().collect();
        let col = extract_str_from_args(&args, 0)?;
        match func_str.as_str() {
            "asc" => {
                return Some(OrderBy {
                    column: col,
                    dir: OrderDir::Asc,
                });
            }
            "desc" => {
                return Some(OrderBy {
                    column: col,
                    dir: OrderDir::Desc,
                });
            }
            _ => {}
        }
    }
    None
}

/// Extract `constraints = [...]` from `#[model(...)]`.
fn extract_constraints(node: &ItemStruct) -> Vec<Constraint> {
    let mut result = Vec::new();
    for attr in &node.attrs {
        if !attr.path().is_ident("model") {
            continue;
        }
        if let Ok(list) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) {
            for meta in list {
                if let syn::Meta::NameValue(nv) = meta
                    && nv.path.is_ident("constraints")
                    && let syn::Expr::Array(arr) = &nv.value
                {
                    for elem in &arr.elems {
                        if let Some(c) = parse_constraint_expr(elem) {
                            result.push(c);
                        }
                    }
                }
            }
        }
    }
    result
}

/// Parse a constraint builder chain expression into a `Constraint`.
///
/// Handles:
/// - `CheckConstraint::new("sql").name("n")`
/// - `UniqueConstraint::on(&["a", "b"]).name("n")`
/// - `UniqueConstraint::on(&["a"]).condition("cond").name("n")`
fn parse_constraint_expr(expr: &syn::Expr) -> Option<Constraint> {
    let (base, methods) = flatten_method_chain(expr);

    if let syn::Expr::Call(call) = base {
        let func = &call.func;
        let func_str = quote::quote!(#func).to_string().replace(" ", "");
        let args: Vec<syn::Expr> = call.args.iter().cloned().collect();

        if func_str == "CheckConstraint::new" {
            let sql = extract_str_from_args(&args, 0).unwrap_or_default();
            let name = methods
                .iter()
                .find(|(m, _)| m == "name")
                .and_then(|(_, a)| extract_str_from_args(a, 0))
                .unwrap_or_default();
            return Some(Constraint::Check(CheckConstraint { sql, name }));
        }

        if func_str == "UniqueConstraint::on" {
            let fields = extract_str_slice_from_args(&args, 0).unwrap_or_default();
            let condition = methods
                .iter()
                .find(|(m, _)| m == "condition")
                .and_then(|(_, a)| extract_str_from_args(a, 0));
            let name = methods
                .iter()
                .find(|(m, _)| m == "name")
                .and_then(|(_, a)| extract_str_from_args(a, 0))
                .unwrap_or_default();
            return Some(Constraint::Unique(UniqueConstraint {
                fields,
                condition,
                name,
            }));
        }
    }
    None
}

/// Flatten `A.b(args1).c(args2)` into `(A, [(b, args1), (c, args2)])`.
fn flatten_method_chain(expr: &syn::Expr) -> (&syn::Expr, Vec<(String, Vec<syn::Expr>)>) {
    let mut methods: Vec<(String, Vec<syn::Expr>)> = Vec::new();
    let mut current = expr;
    while let syn::Expr::MethodCall(mc) = current {
        methods.push((mc.method.to_string(), mc.args.iter().cloned().collect()));
        current = &mc.receiver;
    }
    methods.reverse();
    (current, methods)
}

fn extract_str_from_args(args: &[syn::Expr], idx: usize) -> Option<String> {
    if let syn::Expr::Lit(expr_lit) = args.get(idx)?
        && let syn::Lit::Str(s) = &expr_lit.lit
    {
        return Some(s.value());
    }
    None
}

fn extract_str_slice_from_args(args: &[syn::Expr], idx: usize) -> Option<Vec<String>> {
    let arg = args.get(idx)?;
    // Handle `&["a", "b"]` — a reference to an array literal
    let inner = if let syn::Expr::Reference(r) = arg {
        &*r.expr
    } else {
        arg
    };
    if let syn::Expr::Array(arr) = inner {
        let fields: Vec<String> = arr
            .elems
            .iter()
            .filter_map(|e| {
                if let syn::Expr::Lit(el) = e
                    && let syn::Lit::Str(s) = &el.lit
                {
                    return Some(s.value());
                }
                None
            })
            .collect();
        return Some(fields);
    }
    None
}

fn parse_on_delete(value: &Option<String>) -> Result<ReferentialAction> {
    match value.as_deref() {
        None | Some("cascade") => Ok(ReferentialAction::Cascade),
        Some("set_null") => Ok(ReferentialAction::SetNull),
        Some("set_default") => Ok(ReferentialAction::SetDefault),
        Some("restrict") => Ok(ReferentialAction::Restrict),
        Some("no_action") => Ok(ReferentialAction::NoAction),
        Some(other) => bail!("Invalid on_delete value: {}", other),
    }
}

fn extract_foreign_key_target(ty: &Type) -> Option<String> {
    let s = quote::quote!(#ty).to_string().replace(" ", "");
    let inner = s.strip_prefix("ForeignKey<")?.strip_suffix('>')?;
    let last = inner.rsplit("::").next()?;
    Some(last.to_string())
}

fn resolve_model_tables(
    schemas: &mut [TableSchema],
    m2m_relations: &mut [M2MRelation],
    model_tables: &HashMap<String, String>,
) {
    for schema in schemas {
        for col in &mut schema.columns {
            if let Some(fk) = &mut col.references {
                fk.table = model_tables
                    .get(&fk.table)
                    .cloned()
                    .unwrap_or_else(|| fk.table.clone());
            }
        }
    }

    for rel in m2m_relations {
        rel.to_table = model_tables
            .get(&rel.to_table)
            .cloned()
            .unwrap_or_else(|| rel.to_table.clone());
    }
}

fn to_snake_case(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(ch.to_lowercase().next().unwrap());
    }
    out
}

/// Generate a pivot table schema for a M2M relation.
pub(crate) fn generate_pivot_table(rel: &M2MRelation) -> TableSchema {
    let from_table = rel.from_table.clone();
    let to_table = rel.to_table.clone();
    // Same naming as the runtime, so the two cannot drift.
    let (pivot_name, from_col, to_col) = rango_orm_core::related::m2m_pivot(&from_table, &to_table);

    TableSchema {
        table_name: pivot_name.clone(),
        columns: vec![
            rango_orm_core::ColumnDef {
                name: from_col,
                col_type: rango_orm_core::ColumnType::Uuid,
                nullable: false,
                primary_key: false,
                unique: false,
                default: None,
                references: Some(rango_orm_core::FkReference {
                    table: from_table,
                    column: "id".to_string(),
                    on_delete: rango_orm_core::ReferentialAction::Cascade,
                    on_update: rango_orm_core::ReferentialAction::NoAction,
                }),
                index: false,
            },
            rango_orm_core::ColumnDef {
                name: to_col,
                col_type: rango_orm_core::ColumnType::Uuid,
                nullable: false,
                primary_key: false,
                unique: false,
                default: None,
                references: Some(rango_orm_core::FkReference {
                    table: to_table,
                    column: "id".to_string(),
                    on_delete: rango_orm_core::ReferentialAction::Cascade,
                    on_update: rango_orm_core::ReferentialAction::NoAction,
                }),
                index: true,
            },
        ],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::map_field_type;
    use super::scan_models;
    use rango_orm_core::{ColumnType, Constraint};

    fn ty(s: &str) -> syn::Type {
        syn::parse_str(s).expect("valid type string")
    }

    // ── map_field_type ─────────────────────────────────────────────────────────

    #[test]
    fn test_field_bool() {
        assert_eq!(map_field_type(&ty("FieldBool")).unwrap(), ColumnType::Bool);
    }

    #[test]
    fn test_field_uuid() {
        assert_eq!(map_field_type(&ty("FieldUuid")).unwrap(), ColumnType::Uuid);
    }

    #[test]
    fn test_field_text() {
        assert_eq!(map_field_type(&ty("FieldText")).unwrap(), ColumnType::Text);
    }

    #[test]
    fn test_field_varchar_max() {
        assert_eq!(
            map_field_type(&ty("FieldVarchar<1, 100>")).unwrap(),
            ColumnType::Varchar(100),
        );
    }

    #[test]
    fn test_field_password_maps_to_text() {
        assert_eq!(
            map_field_type(&ty("FieldPassword<8, 128>")).unwrap(),
            ColumnType::Text,
        );
    }

    #[test]
    fn test_foreign_key_maps_to_uuid() {
        assert_eq!(
            map_field_type(&ty("ForeignKey<User>")).unwrap(),
            ColumnType::Uuid,
        );
    }

    #[test]
    fn test_auto_pk_fields() {
        assert_eq!(
            map_field_type(&ty("FieldAutoInt")).unwrap(),
            ColumnType::AutoInt
        );
        assert_eq!(
            map_field_type(&ty("FieldAutoBigInt")).unwrap(),
            ColumnType::AutoBigInt
        );
    }

    #[test]
    fn test_unknown_type_is_error() {
        assert!(map_field_type(&ty("SomeUnknownType")).is_err());
    }

    // ── scanner ───────────────────────────────────────────────────────────────

    fn write_temp_models(name: &str, content: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rango_scanner_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("models.rs"), content).unwrap();
        dir
    }

    #[test]
    fn test_scanner_fk_resolves_model_table() {
        let dir = write_temp_models(
            "fk_table",
            r#"
use rango_orm_core::Model;

#[derive(Model)]
#[model(table = "people")]
pub struct Person {
    id: FieldUuid,
    name: FieldText,
}

#[derive(Model)]
pub struct Profile {
    id: FieldUuid,
    person: ForeignKey<Person>,
}
"#,
        );

        let (schemas, _) = scan_models(dir.to_str().unwrap()).unwrap();
        let profile = schemas.iter().find(|s| s.table_name == "profile").unwrap();
        let col = profile.columns.iter().find(|c| c.name == "person").unwrap();
        let fk = col.references.as_ref().unwrap();
        assert_eq!(fk.table, "people");
        assert_eq!(fk.column, "id");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_scanner_invalid_on_delete_is_error() {
        let dir = write_temp_models(
            "bad_on_delete",
            r#"
use rango_orm_core::Model;

#[derive(Model)]
pub struct Person {
    id: FieldUuid,
}

#[derive(Model)]
pub struct Profile {
    id: FieldUuid,
    #[field(on_delete = "bogus")]
    person: ForeignKey<Person>,
}
"#,
        );

        let result = scan_models(dir.to_str().unwrap());
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_scanner_constraints_ordering_and_m2m() {
        let dir = write_temp_models(
            "constraints_m2m",
            r#"
use rango_orm_core::Model;

#[derive(Model)]
#[model(
    constraints = [
        CheckConstraint::new("age >= 0").name("chk_age"),
        UniqueConstraint::on(&["name", "age"]).name("uniq_name_age"),
    ],
    ordering = [desc("age")]
)]
pub struct Person {
    id: FieldUuid,
    name: FieldText,
    age: FieldInt,
}

#[derive(Model)]
pub struct Post {
    id: FieldUuid,
    readers: ManyToMany<Person>,
}
"#,
        );

        let (schemas, m2m) = scan_models(dir.to_str().unwrap()).unwrap();
        let person = schemas.iter().find(|s| s.table_name == "person").unwrap();
        assert_eq!(person.constraints.len(), 2);
        assert!(matches!(&person.constraints[0], Constraint::Check(c) if c.sql == "age >= 0"));
        assert!(
            matches!(&person.constraints[1], Constraint::Unique(u) if u.fields == ["name", "age"])
        );
        assert_eq!(person.ordering.len(), 1);
        assert_eq!(person.ordering[0].column, "age");

        let post = schemas.iter().find(|s| s.table_name == "post").unwrap();
        assert!(post.columns.iter().all(|c| c.name != "readers"));
        assert_eq!(m2m.len(), 1);
        assert_eq!(m2m[0].to_table, "person");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_scanner_implicit_pk_attribute_macro() {
        let dir = write_temp_models(
            "implicit_pk",
            r#"
use rango::prelude::*;

#[rango::model]
#[model(table = "art")]
pub struct Article {
    title: FieldText,
}

#[rango::model(pk = "bigint")]
pub struct Event {
    name: FieldText,
}

#[model]
pub struct Plain {
    name: FieldText,
}

#[rango::model]
pub struct Keeps {
    id: FieldAutoBigInt,
}

#[rango::model]
pub struct Custom {
    #[field(primary_key)]
    code: FieldText,
}

#[derive(Model)]
#[model(table = "helper_only")]
pub struct Helper {
    id: FieldUuid,
}
"#,
        );

        let (schemas, _) = scan_models(dir.to_str().unwrap()).unwrap();
        let col = |t: &str| {
            let s = schemas.iter().find(|s| s.table_name == t).unwrap();
            (s.columns.len(), s.columns[0].clone())
        };
        let (n, id) = col("art");
        assert_eq!((n, id.name.as_str(), id.primary_key), (2, "id", true));
        assert_eq!(id.col_type, ColumnType::Uuid);
        assert_eq!(col("event").1.col_type, ColumnType::AutoBigInt);
        assert_eq!(col("plain").1.col_type, ColumnType::Uuid);
        assert_eq!(col("keeps").0, 1);
        assert_eq!(col("custom").0, 1);
        assert_eq!(col("helper_only").0, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_scanner_m2m_uses_explicit_tables_and_runtime_pivot_name() {
        let dir = write_temp_models(
            "m2m_explicit",
            r#"
use rango_orm_core::Model;

#[derive(Model)]
#[model(table = "people")]
pub struct Person {
    id: FieldUuid,
}

#[derive(Model)]
#[model(table = "articles")]
pub struct Post {
    id: FieldUuid,
    readers: ManyToMany<Person>,
}
"#,
        );
        let (_, m2m) = scan_models(dir.to_str().unwrap()).unwrap();
        let pivot = super::generate_pivot_table(&m2m[0]);
        assert_eq!(pivot.table_name, "articles_people");
        assert_eq!(pivot.columns[0].name, "articles_id");
        assert_eq!(
            pivot.columns[1].references.as_ref().unwrap().table,
            "people"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
