//! Struct-level `#[model(...)]` options: `managed`, `ordering`, `constraints`.

use super::*;

/// Extract `managed = false` from `#[model(...)]`, defaults to `true`.
pub(super) fn extract_managed(node: &ItemStruct) -> bool {
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
pub(super) fn extract_ordering(node: &ItemStruct) -> Vec<OrderBy> {
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
pub(super) fn parse_order_expr(expr: &syn::Expr) -> Option<OrderBy> {
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
pub(super) fn extract_constraints(node: &ItemStruct) -> Vec<Constraint> {
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
pub(super) fn parse_constraint_expr(expr: &syn::Expr) -> Option<Constraint> {
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
pub(super) fn flatten_method_chain(
    expr: &syn::Expr,
) -> (&syn::Expr, Vec<(String, Vec<syn::Expr>)>) {
    let mut methods: Vec<(String, Vec<syn::Expr>)> = Vec::new();
    let mut current = expr;
    while let syn::Expr::MethodCall(mc) = current {
        methods.push((mc.method.to_string(), mc.args.iter().cloned().collect()));
        current = &mc.receiver;
    }
    methods.reverse();
    (current, methods)
}

pub(super) fn extract_str_from_args(args: &[syn::Expr], idx: usize) -> Option<String> {
    if let syn::Expr::Lit(expr_lit) = args.get(idx)?
        && let syn::Lit::Str(s) = &expr_lit.lit
    {
        return Some(s.value());
    }
    None
}

pub(super) fn extract_str_slice_from_args(args: &[syn::Expr], idx: usize) -> Option<Vec<String>> {
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
