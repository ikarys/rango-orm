//! Database-free core of [Rango ORM](https://crates.io/crates/rango-orm): model traits, schema and field types.
//!
//! Internal crate: depend on `rango-orm`, which re-exports it.
#![forbid(unsafe_code)]
pub mod backend;
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

pub use backend::RangoBackend;
pub use column::{
    Column, ColumnRef, FilterColumn, FilterValue, ForeignKeyRef, ManyToManyField, OrderRef,
    Ordering, PrefetchKind, PrefetchRef,
};
pub use config::{BackendKind, DatabaseConfig};
pub use constraints::{
    CheckConstraint, Constraint, IndexDef, OrderBy, OrderDir, UniqueConstraint, asc,
    check_constraint_name, constraint_definition, constraint_name, desc, unique_constraint_name,
};
pub use error::{Error, Result};
pub use fields::*;
pub use hooks::ModelHooks;
pub use model::{Model, ModelMixin, unknown_column};
pub use query::{
    Filterable, ModelValues, SqlValue, ToSqlValue, insert_values, quote_ident, quote_ident_mysql,
};
pub use related::WithRelated;
pub use row::{FromRow, RangoRow, RowError};
pub use schema::{
    ColumnDef, ColumnType, DefaultValue, FkReference, ReferentialAction, TableSchema,
};
