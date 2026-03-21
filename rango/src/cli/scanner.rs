use anyhow::{Context, Result};
use rango_core::{
    CheckConstraint, ColumnDef, ColumnType, Constraint, DefaultValue,
    OrderBy, OrderDir, TableSchema, UniqueConstraint,
};
use std::path::Path;
use syn::{visit::Visit, File, ItemStruct, Type};
use walkdir::WalkDir;

/// Scan a source directory and return (schemas, m2m_relations).
pub fn scan_models(src_dir: &str) -> Result<(Vec<TableSchema>, Vec<M2MRelation>)> {
    let mut schemas = Vec::new();
    let mut m2m_relations = Vec::new();

    for entry in WalkDir::new(src_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "rs").unwrap_or(false))
        .filter(|e| {
            let name = e.path().file_name().unwrap_or_default();
            !matches!(name.to_str().unwrap_or(""), "mod.rs" | "main.rs" | "lib.rs")
        })
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
    }

    Ok((schemas, m2m_relations))
}

/// Quick check: does this file import rango?
fn uses_rango(content: &str) -> bool {
    content.contains("rango_derive")
        || content.contains("rango_core")
        || content.contains("use rango")
}

/// A detected M2M relation between two table names.
#[derive(Debug, Clone)]
pub struct M2MRelation {
    pub from_table: String, // will be prefixed later
    pub to_table: String,   // will be prefixed later
}

struct ModelVisitor<'a> {
    path: &'a Path,
    pub schemas: Vec<TableSchema>,
    pub m2m_relations: Vec<M2MRelation>,
}

impl<'a> ModelVisitor<'a> {
    fn new(path: &'a Path) -> Self {
        Self { path, schemas: Vec::new(), m2m_relations: Vec::new() }
    }
}

impl<'ast> Visit<'ast> for ModelVisitor<'_> {
    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        if !has_derive_model(node) {
            return;
        }

        let table_name = extract_table_name(node);
        let mut columns = Vec::new();

        if let syn::Fields::Named(fields) = &node.fields {
            for field in &fields.named {
                // Detect ManyToMany<T> — record relation, skip column
                let type_str = quote::quote!(#(field.ty)).to_string().replace(" ", "");
                if type_str.starts_with("ManyToMany<") {
                    let to_model = type_str
                        .trim_start_matches("ManyToMany<")
                        .trim_end_matches('>')
                        .to_string();
                    let to_table = to_snake_case(&to_model);
                    self.m2m_relations.push(M2MRelation {
                        from_table: table_name.clone(),
                        to_table,
                    });
                    continue;
                }

                match build_column_def(field) {
                    Ok(col) => columns.push(col),
                    Err(e) => eprintln!(
                        "Warning: skipping field in {} ({}): {}",
                        self.path.display(), node.ident, e
                    ),
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

/// Check if a struct has `#[derive(Model)]`
fn has_derive_model(node: &ItemStruct) -> bool {
    for attr in &node.attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }
        if let Ok(list) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated
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
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
        ) {
            for meta in list {
                if let syn::Meta::NameValue(nv) = meta {
                    if nv.path.is_ident("table") {
                        if let syn::Expr::Lit(expr_lit) = &nv.value {
                            if let syn::Lit::Str(s) = &expr_lit.lit {
                                return s.value();
                            }
                        }
                    }
                }
            }
        }
    }
    to_snake_case(&node.ident.to_string())
}

/// Build a ColumnDef from a struct field
fn build_column_def(field: &syn::Field) -> Result<ColumnDef> {
    let field_name = field.ident.as_ref()
        .ok_or_else(|| anyhow::anyhow!("unnamed field"))?
        .to_string();

    let field_attrs = parse_field_attrs(field);

    let col_name = field_attrs.column.unwrap_or_else(|| field_name.clone());
    let is_pk = field_attrs.primary_key || field_name == "id";

    let (nullable, inner_ty) = extract_option(&field.ty);
    let col_type = map_field_type(inner_ty)?;

    let default = if field_attrs.auto_now_add || field_attrs.auto_now {
        Some(DefaultValue::CurrentTimestamp)
    } else if let Some(d) = field_attrs.default {
        Some(DefaultValue::Literal(d))
    } else {
        None
    };

    Ok(ColumnDef {
        name: col_name,
        col_type,
        nullable,
        primary_key: is_pk,
        unique: is_pk || field_attrs.unique,
        default,
        references: None,
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
}

fn parse_field_attrs(field: &syn::Field) -> FieldAttrs {
    let mut attrs = FieldAttrs::default();
    for attr in &field.attrs {
        if !attr.path().is_ident("field") {
            continue;
        }
        if let Ok(list) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
        ) {
            for meta in list {
                match &meta {
                    syn::Meta::Path(p) if p.is_ident("unique")       => attrs.unique = true,
                    syn::Meta::Path(p) if p.is_ident("index")        => attrs.index = true,
                    syn::Meta::Path(p) if p.is_ident("primary_key")  => attrs.primary_key = true,
                    syn::Meta::Path(p) if p.is_ident("auto_now_add") => attrs.auto_now_add = true,
                    syn::Meta::Path(p) if p.is_ident("auto_now")     => attrs.auto_now = true,
                    syn::Meta::NameValue(nv) if nv.path.is_ident("column") => {
                        if let syn::Expr::Lit(e) = &nv.value {
                            if let syn::Lit::Str(s) = &e.lit { attrs.column = Some(s.value()); }
                        }
                    }
                    syn::Meta::NameValue(nv) if nv.path.is_ident("default") => {
                        if let syn::Expr::Lit(e) = &nv.value {
                            if let syn::Lit::Str(s) = &e.lit { attrs.default = Some(s.value()); }
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

fn map_field_type(ty: &Type) -> Result<ColumnType> {
    let s = quote::quote!(#ty).to_string().replace(" ", "");
    let col = match s.as_str() {
        "FieldBool"       => ColumnType::Bool,
        "FieldSmallInt"   => ColumnType::SmallInt,
        "FieldInt"        => ColumnType::Int,
        "FieldBigInt"     => ColumnType::BigInt,
        "FieldFloat"      => ColumnType::Float,
        "FieldDouble"     => ColumnType::Double,
        "FieldText"       => ColumnType::Text,
        "FieldEmail"      => ColumnType::Varchar(254),
        "FieldUrl"        => ColumnType::Varchar(2048),
        "FieldBytes"      => ColumnType::Bytea,
        "FieldUuid"       => ColumnType::Uuid,
        "FieldDate"       => ColumnType::Date,
        "FieldTime"       => ColumnType::Time,
        "FieldDateTime"   => ColumnType::DateTime,
        "FieldJson"       => ColumnType::Jsonb,
        s if s.starts_with("FieldVarchar<") => parse_varchar(s)?,
        s if s.starts_with("FieldPassword<") => parse_password(s)?,
        s if s.starts_with("FieldDecimal<") => parse_decimal(s)?,
        s if s.starts_with("FieldRange<") => ColumnType::BigInt,
        _ => anyhow::bail!("Unknown field type: {}", s),
    };
    Ok(col)
}

fn parse_varchar(s: &str) -> Result<ColumnType> {
    let inner = s.trim_start_matches("FieldVarchar<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 {
        if let Ok(max) = parts[1].trim().parse::<u32>() {
            return Ok(ColumnType::Varchar(max));
        }
    }
    anyhow::bail!("Invalid FieldVarchar: {}", s)
}

fn parse_password(s: &str) -> Result<ColumnType> {
    let inner = s.trim_start_matches("FieldPassword<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 {
        if let Ok(max) = parts[1].trim().parse::<u32>() {
            return Ok(ColumnType::Varchar(max));
        }
    }
    anyhow::bail!("Invalid FieldPassword: {}", s)
}

fn parse_decimal(s: &str) -> Result<ColumnType> {
    let inner = s.trim_start_matches("FieldDecimal<").trim_end_matches('>');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 2 {
        if let (Ok(p), Ok(sc)) = (parts[0].trim().parse::<u8>(), parts[1].trim().parse::<u8>()) {
            return Ok(ColumnType::Decimal { precision: p, scale: sc });
        }
    }
    anyhow::bail!("Invalid FieldDecimal: {}", s)
}

/// Extract `managed = false` from `#[model(...)]`, defaults to `true`.
fn extract_managed(node: &ItemStruct) -> bool {
    for attr in &node.attrs {
        if !attr.path().is_ident("model") { continue; }
        if let Ok(list) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
        ) {
            for meta in list {
                if let syn::Meta::NameValue(nv) = meta {
                    if nv.path.is_ident("managed") {
                        if let syn::Expr::Lit(expr_lit) = &nv.value {
                            if let syn::Lit::Bool(b) = &expr_lit.lit {
                                return b.value;
                            }
                        }
                    }
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
        if !attr.path().is_ident("model") { continue; }
        if let Ok(list) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
        ) {
            for meta in list {
                if let syn::Meta::NameValue(nv) = meta {
                    if nv.path.is_ident("ordering") {
                        if let syn::Expr::Array(arr) = &nv.value {
                            for elem in &arr.elems {
                                if let Some(ob) = parse_order_expr(elem) {
                                    result.push(ob);
                                }
                            }
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
        let func_str = quote::quote!(#(call.func)).to_string().replace(" ", "");
        let args: Vec<syn::Expr> = call.args.iter().cloned().collect();
        let col = extract_str_from_args(&args, 0)?;
        match func_str.as_str() {
            "asc"  => return Some(OrderBy { column: col, dir: OrderDir::Asc }),
            "desc" => return Some(OrderBy { column: col, dir: OrderDir::Desc }),
            _ => {}
        }
    }
    None
}

/// Extract `constraints = [...]` from `#[model(...)]`.
fn extract_constraints(node: &ItemStruct) -> Vec<Constraint> {
    let mut result = Vec::new();
    for attr in &node.attrs {
        if !attr.path().is_ident("model") { continue; }
        if let Ok(list) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
        ) {
            for meta in list {
                if let syn::Meta::NameValue(nv) = meta {
                    if nv.path.is_ident("constraints") {
                        if let syn::Expr::Array(arr) = &nv.value {
                            for elem in &arr.elems {
                                if let Some(c) = parse_constraint_expr(elem) {
                                    result.push(c);
                                }
                            }
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
        let func_str = quote::quote!(#(call.func)).to_string().replace(" ", "");
        let args: Vec<syn::Expr> = call.args.iter().cloned().collect();

        if func_str == "CheckConstraint::new" {
            let sql = extract_str_from_args(&args, 0).unwrap_or_default();
            let name = methods.iter()
                .find(|(m, _)| m == "name")
                .and_then(|(_, a)| extract_str_from_args(a, 0))
                .unwrap_or_default();
            return Some(Constraint::Check(CheckConstraint { sql, name }));
        }

        if func_str == "UniqueConstraint::on" {
            let fields = extract_str_slice_from_args(&args, 0).unwrap_or_default();
            let condition = methods.iter()
                .find(|(m, _)| m == "condition")
                .and_then(|(_, a)| extract_str_from_args(a, 0));
            let name = methods.iter()
                .find(|(m, _)| m == "name")
                .and_then(|(_, a)| extract_str_from_args(a, 0))
                .unwrap_or_default();
            return Some(Constraint::Unique(UniqueConstraint { fields, condition, name }));
        }
    }
    None
}

/// Flatten `A.b(args1).c(args2)` into `(A, [(b, args1), (c, args2)])`.
fn flatten_method_chain(expr: &syn::Expr) -> (&syn::Expr, Vec<(String, Vec<syn::Expr>)>) {
    let mut methods: Vec<(String, Vec<syn::Expr>)> = Vec::new();
    let mut current = expr;
    loop {
        if let syn::Expr::MethodCall(mc) = current {
            methods.push((mc.method.to_string(), mc.args.iter().cloned().collect()));
            current = &mc.receiver;
        } else {
            break;
        }
    }
    methods.reverse();
    (current, methods)
}

fn extract_str_from_args(args: &[syn::Expr], idx: usize) -> Option<String> {
    if let syn::Expr::Lit(expr_lit) = args.get(idx)? {
        if let syn::Lit::Str(s) = &expr_lit.lit {
            return Some(s.value());
        }
    }
    None
}

fn extract_str_slice_from_args(args: &[syn::Expr], idx: usize) -> Option<Vec<String>> {
    let arg = args.get(idx)?;
    // Handle `&["a", "b"]` — a reference to an array literal
    let inner = if let syn::Expr::Reference(r) = arg { &*r.expr } else { arg };
    if let syn::Expr::Array(arr) = inner {
        let fields: Vec<String> = arr.elems.iter().filter_map(|e| {
            if let syn::Expr::Lit(el) = e {
                if let syn::Lit::Str(s) = &el.lit { return Some(s.value()); }
            }
            None
        }).collect();
        return Some(fields);
    }
    None
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
