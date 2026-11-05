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
pub mod row;
pub mod schema;

pub use backend::RangoBackend;
pub use column::{Column, ColumnRef, FilterColumn, FilterValue, OrderRef, Ordering};
pub use config::{BackendKind, DatabaseConfig};
pub use constraints::{
    CheckConstraint, Constraint, IndexDef, OrderBy, OrderDir, UniqueConstraint, asc,
    check_constraint_name, constraint_definition, constraint_name, desc, unique_constraint_name,
};
pub use error::{Error, Result};
pub use fields::*;
pub use hooks::ModelHooks;
pub use model::{Model, ModelMixin};
pub use query::{Filterable, ModelValues, SqlValue, ToSqlValue, quote_ident, quote_ident_mysql};
pub use row::{FromRow, RangoRow, RowError};
pub use schema::{
    ColumnDef, ColumnType, DefaultValue, FkReference, ReferentialAction, TableSchema,
};
