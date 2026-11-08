use anyhow::{Context, Result, bail};
use rango_orm_core::type_map::{self, TypeMapError};
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
                let type_str = type_name(ty);
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
                if path.segments.last().is_some_and(|s| s.ident == "Model") {
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

mod attrs;
mod fields;
mod relations;
#[cfg(test)]
mod tests;

use attrs::{extract_constraints, extract_managed, extract_ordering};
use fields::{build_column_def, parse_field_attrs, type_name};
pub(crate) use relations::{fk_type, generate_pivot_table};
use relations::{resolve_model_tables, to_snake_case};
