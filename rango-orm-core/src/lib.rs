//! Database-free core of [Rango ORM](https://crates.io/crates/rango-orm): model traits, schema and field types.
//!
//! Internal crate: depend on `rango-orm`, which re-exports it.
#![forbid(unsafe_code)]
pub mod column;
pub mod config;
pub mod constraints;
pub mod error;
pub mod fields;
pub mod hooks;
pub mod model;
pub mod query;
pub mod related;
pub mod row;
pub mod schema;
#[doc(hidden)]
pub mod type_map;

pub use column::{
    Column, ColumnRef, FilterColumn, FilterValue, ForeignKeyRef, ManyToManyField, OrderRef,
    Ordering, PrefetchKind, PrefetchRef,
};
pub use config::{BackendKind, DatabaseConfig};
pub use constraints::{
    CheckConstraint, Constraint, IndexDef, OrderBy, OrderDir, UniqueConstraint, asc, desc,
};
// Migration / backend plumbing, not part of the user API.
#[doc(hidden)]
pub use constraints::{
    check_constraint_name, constraint_definition, constraint_name, unique_constraint_name,
};
pub use error::{Error, Result};
pub use fields::*;
pub use hooks::{BoxFuture, HookResult, ModelHooks};
#[doc(hidden)]
pub use hooks::{
    after_delete, after_insert, after_update, before_delete, before_insert, before_update,
};
pub use model::{Model, ModelMixin, PrimaryKey, PrimaryKeyField, UnsupportedPk, unknown_column};
#[doc(hidden)]
pub use query::insert_values;
pub use query::{Filterable, ModelValues, SqlValue, ToSqlValue, quote_ident, quote_ident_mysql};
pub use related::WithRelated;
pub use row::{FromRow, Partial, RangoRow, RowError};
pub use rust_decimal::Decimal;
pub use schema::{
    ColumnDef, ColumnType, DefaultValue, FkReference, ReferentialAction, TableSchema,
};
