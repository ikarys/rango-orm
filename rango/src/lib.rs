//! Rango ORM — re-exports everything the user needs.
//!
//! # Quick start
//!
//! Add to `Cargo.toml`:
//! ```toml
//! rango = "0.1"                                                        # postgres (default)
//! # or
//! rango = { version = "0.1", default-features = false, features = ["sqlite"] }
//! ```
//!
//! Then:
//! ```rust,ignore
//! use rango::prelude::*;
//!
//! #[derive(Model)]
//! struct User {
//!     id: FieldUuid,
//!     email: FieldEmail,
//! }
//! ```
#![forbid(unsafe_code)]
// Re-export core types — always available
pub use rango_core::*;

// Make rango_core visible for proc-macro generated code.
#[doc(hidden)]
pub extern crate rango_core;

// Re-export derive macros
pub use rango_derive::Model;
pub use rango_derive::ModelMixin;

// ─── Postgres backend (feature `postgres`, enabled by default) ───────────────

#[cfg(feature = "postgres")]
pub use rango_postgres::PgPool;
#[cfg(feature = "postgres")]
pub use rango_postgres::{
    M2M, Pg, PgQueryExt, QueryBuilder, RangoExecutor, RangoFilterExt, RangoTransaction, TxFuture,
    WithRelated, all, atomic, atomic_retry, delete, get, get_or_create, insert, raw, raw_execute,
    raw_scalar, transaction, update, update_or_create,
};

#[cfg(feature = "postgres")]
pub async fn connect_postgres(config: &DatabaseConfig) -> Result<PgPool> {
    rango_postgres::connect(config).await
}

// ─── SQLite backend (feature `sqlite`) ────────────────────────────────────────

#[cfg(feature = "sqlite")]
pub use rango_sqlite::SqlitePool;

/// SQLite-specific ops — use these when working with a SqlitePool.
/// Parallel to the postgres ops but for SQLite.
#[cfg(feature = "sqlite")]
pub mod sqlite {
    pub use rango_sqlite::{
        QueryBuilder, RangoFilterExt, SqliteTransaction, TxFuture, all, atomic, bulk_create,
        delete, get, get_or_create, insert, raw, raw_execute, raw_scalar, update,
    };
}

#[cfg(feature = "sqlite")]
pub async fn connect_sqlite(url: &str) -> Result<SqlitePool> {
    rango_sqlite::connect(url).await
}

// ─── Prelude ──────────────────────────────────────────────────────────────────

pub mod prelude {
    pub use rango_core::ModelHooks;
    pub use rango_core::*;
    pub use rango_derive::{Model, ModelMixin};

    #[cfg(feature = "postgres")]
    pub use rango_postgres::{
        M2M, Pg, PgPool, PgQueryExt, QueryBuilder, RangoExecutor, RangoFilterExt, RangoTransaction,
        TxFuture, WithRelated, all, atomic, atomic_retry, delete, get, get_or_create, insert, raw,
        raw_execute, raw_scalar, transaction, update, update_or_create,
    };

    #[cfg(feature = "sqlite")]
    pub use crate::sqlite::*;
    #[cfg(feature = "sqlite")]
    pub use rango_sqlite::{SqlitePool, SqliteQueryExt};
}
