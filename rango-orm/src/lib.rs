//! Rango ORM — re-exports everything the user needs.
//!
//! # Quick start
//!
//! Add to `Cargo.toml`:
//! ```toml
//! rango-orm = "0.1"                                                     # postgres (default)
//! # or
//! rango-orm = { version = "0.1", default-features = false, features = ["sqlite"] }
//! # or MySQL / MariaDB:
//! rango-orm = { version = "0.1", default-features = false, features = ["mysql"] }
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
//!
//! # Typed filters
//!
//! `#[derive(Model)]` generates one typed column per field. Filters check the
//! column *and* the value type at compile time:
//!
//! ```rust,no_run
//! # #[cfg(feature = "postgres")] {
//! use rango::prelude::*;
//!
//! #[derive(Model)]
//! struct User {
//!     id: FieldUuid,
//!     email: FieldEmail,
//!     age: FieldInt,
//!     score: Option<FieldBigInt>,
//! }
//!
//! fn adults(pool: &PgPool) -> QueryBuilder<User> {
//!     User::filter(pool)
//!         .gte(User::AGE, 18)
//!         .gt(User::SCORE, 100)
//!         .eq(User::EMAIL, "a@b.c")
//!         .order_by(User::AGE.desc())
//! }
//! # }
//! ```
//!
//! A value of the wrong type does not compile:
//!
//! ```rust,compile_fail
//! # #[cfg(feature = "postgres")] {
//! use rango::prelude::*;
//!
//! #[derive(Model)]
//! struct User {
//!     id: FieldUuid,
//!     age: FieldInt,
//! }
//!
//! fn broken(pool: &PgPool) -> QueryBuilder<User> {
//!     User::filter(pool).eq(User::AGE, "text")
//! }
//! # }
//! # #[cfg(not(feature = "postgres"))] compile_error!("needs the postgres feature");
//! ```
#![forbid(unsafe_code)]
// Re-export core types — always available
pub use rango_orm_core::*;

// Make rango_orm_core visible for proc-macro generated code.
#[doc(hidden)]
pub extern crate rango_orm_core;

// Re-export derive macros
pub use rango_orm_derive::Model;
pub use rango_orm_derive::ModelMixin;

// ─── Postgres backend (feature `postgres`, enabled by default) ───────────────

#[cfg(feature = "postgres")]
pub use rango_orm_postgres::PgPool;
#[cfg(feature = "postgres")]
pub use rango_orm_postgres::{
    M2M, Pg, PgQueryExt, QueryBuilder, RangoExecutor, RangoFilterExt, RangoTransaction, TxFuture,
    WithRelated, all, atomic, atomic_retry, bulk_create, bulk_update, bulk_upsert, delete, get,
    get_or_create, insert, raw, raw_execute, raw_scalar, transaction, update, update_or_create,
};

#[cfg(feature = "postgres")]
pub async fn connect_postgres(config: &DatabaseConfig) -> Result<PgPool> {
    rango_orm_postgres::connect(config).await
}

// ─── SQLite backend (feature `sqlite`) ────────────────────────────────────────

#[cfg(feature = "sqlite")]
pub use rango_orm_sqlite::SqlitePool;

/// SQLite-specific ops — use these when working with a SqlitePool.
/// Parallel to the postgres ops but for SQLite.
#[cfg(feature = "sqlite")]
pub mod sqlite {
    pub use rango_orm_sqlite::{
        M2M, QueryBuilder, RangoFilterExt, SqliteTransaction, TxFuture, all, atomic, bulk_create,
        delete, get, get_or_create, insert, raw, raw_execute, raw_scalar, update,
    };
}

#[cfg(feature = "sqlite")]
pub async fn connect_sqlite(url: &str) -> Result<SqlitePool> {
    rango_orm_sqlite::connect(url).await
}

// ─── MySQL / MariaDB backend (feature `mysql`) ───────────────────────────────

#[cfg(feature = "mysql")]
pub use rango_orm_mysql::MySqlPool;

/// MySQL 8.0+ / MariaDB 10.6+ ops — use these when working with a MySqlPool.
#[cfg(feature = "mysql")]
pub mod mysql {
    pub use rango_orm_mysql::{
        M2M, MySqlTransaction, QueryBuilder, RangoFilterExt, TxFuture, all, atomic, bulk_create,
        delete, get, get_or_create, insert, raw, raw_execute, raw_scalar, update,
    };
}

#[cfg(feature = "mysql")]
pub async fn connect_mysql(url: &str) -> Result<MySqlPool> {
    rango_orm_mysql::connect(url).await
}

// ─── Prelude ──────────────────────────────────────────────────────────────────

pub mod prelude {
    pub use rango_orm_core::ModelHooks;
    pub use rango_orm_core::*;
    pub use rango_orm_derive::{Model, ModelMixin};

    #[cfg(feature = "postgres")]
    pub use rango_orm_postgres::{
        M2M, Pg, PgPool, PgQueryExt, QueryBuilder, RangoExecutor, RangoFilterExt, RangoTransaction,
        TxFuture, WithRelated, all, atomic, atomic_retry, bulk_create, bulk_update, bulk_upsert,
        delete, get, get_or_create, insert, raw, raw_execute, raw_scalar, transaction, update,
        update_or_create,
    };

    #[cfg(feature = "sqlite")]
    pub use crate::sqlite::*;
    #[cfg(feature = "sqlite")]
    pub use rango_orm_sqlite::{SqlitePool, SqliteQueryExt};

    #[cfg(feature = "mysql")]
    pub use crate::mysql::*;
    #[cfg(feature = "mysql")]
    pub use rango_orm_mysql::MySqlPool;
}
