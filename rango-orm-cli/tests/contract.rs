//! Derive <-> CLI contract: the schema `#[derive(Model)]` compiles into and the schema
//! `rango makemigrations` scans out of the same source file must be identical.
//! No database needed.

mod common;
#[path = "fixtures/models.rs"]
mod models;

use common::{Project, assert_ok};
use rango::prelude::*;
use rango_orm_core::constraints::{constraint_definition, constraint_name};
use rango_orm_core::related::m2m_pivot;
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn col_type(t: &ColumnType) -> String {
    match t {
        ColumnType::Bool => "bool".into(),
        ColumnType::SmallInt => "smallint".into(),
        ColumnType::Int => "int".into(),
        ColumnType::BigInt => "bigint".into(),
        ColumnType::AutoInt => "autoint".into(),
        ColumnType::AutoBigInt => "autobigint".into(),
        ColumnType::Float => "float".into(),
        ColumnType::Double => "double".into(),
        ColumnType::Text => "text".into(),
        ColumnType::Bytea => "bytea".into(),
        ColumnType::Uuid => "uuid".into(),
        ColumnType::Date => "date".into(),
        ColumnType::Time => "time".into(),
        ColumnType::DateTime => "datetime".into(),
        ColumnType::Json => "json".into(),
        ColumnType::Jsonb => "jsonb".into(),
        ColumnType::Varchar(n) => format!("varchar({n})"),
        ColumnType::Decimal { precision, scale } => format!("decimal({precision},{scale})"),
    }
}

fn action(a: &ReferentialAction) -> &'static str {
    match a {
        ReferentialAction::Cascade => "cascade",
        ReferentialAction::SetNull => "set_null",
        ReferentialAction::SetDefault => "set_default",
        ReferentialAction::Restrict => "restrict",
        ReferentialAction::NoAction => "no_action",
    }
}

/// What the compiled derive says about a table, in the snapshot's JSON shape.
/// `default` is reduced to "has one" (the CLI serialises the literal in its own syntax).
fn expected(s: &TableSchema) -> Value {
    let columns: Vec<Value> = s
        .columns
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "col_type": col_type(&c.col_type),
                "nullable": c.nullable,
                "primary_key": c.primary_key,
                "unique": c.unique,
                "index": c.index,
                "has_default": c.default.is_some(),
                "references": c.references.as_ref().map(|r| json!({
                    "table": r.table, "column": r.column, "on_delete": action(&r.on_delete),
                })),
            })
        })
        .collect();
    let constraints: Vec<Value> = s
        .constraints
        .iter()
        .enumerate()
        .map(|(i, c)| {
            json!({"name": constraint_name(&s.table_name, c, i), "definition": constraint_definition(c)})
        })
        .collect();
    json!({"columns": columns, "constraints": constraints})
}

/// The same shape, read from `snapshot.json`.
fn scanned(t: &Value) -> Value {
    let columns: Vec<Value> = t["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            json!({
                "name": c["name"],
                "col_type": c["col_type"],
                "nullable": c["nullable"],
                "primary_key": c["primary_key"],
                "unique": c["unique"],
                "index": c["index"],
                "has_default": !c["default"].is_null(),
                "references": c["references"],
            })
        })
        .collect();
    json!({"columns": columns, "constraints": t["constraints"]})
}

fn pivot_schema(from: &str, to: &str) -> TableSchema {
    let (name, from_col, to_col) = m2m_pivot(from, to);
    let col = |n: String, t: &str, index: bool| ColumnDef {
        name: n,
        col_type: ColumnType::Uuid,
        nullable: false,
        primary_key: false,
        unique: false,
        default: None,
        index,
        references: Some(FkReference {
            table: t.to_string(),
            column: "id".into(),
            on_delete: ReferentialAction::Cascade,
            on_update: ReferentialAction::NoAction,
        }),
    };
    TableSchema {
        table_name: name,
        // The pivot's composite key covers the first column; the second gets its own index.
        columns: vec![col(from_col, from, false), col(to_col, to, true)],
        ..TableSchema::default()
    }
}

fn snapshot_of(backend: &str) -> Value {
    let p = Project::new(&format!("contract_{backend}"), backend);
    assert_ok(&p.cli(None, &["makemigrations", "src", "--yes"]));
    p.snapshot()
}

#[test]
fn scanner_matches_derive_for_every_model() {
    // The scan is backend independent (only the SQL differs): one backend is enough here.
    let snap = snapshot_of("sqlite");
    let tables = snap["tables"].as_object().unwrap();

    let all = models::schemas();
    let (managed, unmanaged): (Vec<_>, Vec<_>) = all.iter().partition(|s| s.managed);
    assert!(!unmanaged.is_empty(), "fixture must cover managed = false");

    // M2M declared in the fixture: RpxBlogPost.tags -> RpxTag and the self-referencing
    // RpxAuthor.followers -> RpxAuthor (pivot columns `from_..._id` / `to_..._id`).
    let pivots = [
        pivot_schema(
            models::RpxBlogPost::table_name(),
            models::RpxTag::table_name(),
        ),
        pivot_schema(
            models::RpxAuthor::table_name(),
            models::RpxAuthor::table_name(),
        ),
    ];

    let want: BTreeSet<&str> = managed
        .iter()
        .map(|s| s.table_name.as_str())
        .chain(pivots.iter().map(|s| s.table_name.as_str()))
        .collect();
    let got: BTreeSet<&str> = tables.keys().map(String::as_str).collect();
    assert_eq!(
        got, want,
        "tables the CLI will create vs tables the derive names"
    );

    let mut problems = Vec::new();
    for s in managed.iter().copied().chain(pivots.iter()) {
        let (e, g) = (expected(s), scanned(&tables[&s.table_name]));
        if e != g {
            problems.push(format!(
                "table `{}`:\n  derive: {}\n  CLI:    {}",
                s.table_name, e, g
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Derive tables the CLI would never migrate must not leak in as models (mixins, partials).
#[test]
fn non_models_are_not_tables() {
    let snap = snapshot_of("sqlite");
    let tables = snap["tables"].as_object().unwrap();
    for name in ["rpx_timestamps", "rpx_author_lite", "rpx_legacy"] {
        assert!(!tables.contains_key(name), "{name} must not be migrated");
    }
}

/// The SQL generated for each backend must at least mention every table once.
#[test]
fn makemigrations_covers_every_backend() {
    for backend in ["sqlite", "postgres", "mysql"] {
        let p = Project::new(&format!("contract_sql_{backend}"), backend);
        assert_ok(&p.cli(None, &["makemigrations", "src", "--yes"]));
        let sql = p.migrations();
        for t in p.snapshot()["tables"].as_object().unwrap().keys() {
            assert!(sql.contains(t.as_str()), "{backend}: no SQL for {t}");
        }
    }
}
