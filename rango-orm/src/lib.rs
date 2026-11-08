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
//! Rango does not re-export `uuid` or `chrono`: add them to your own
//! `Cargo.toml` to build `FieldUuid` / `FieldDateTime` values.
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
//!
//! # Partial selects
//!
//! `#[derive(Partial)]` declares a struct holding some of a model's columns;
//! `select_as` fetches only those columns. A field whose name or type does not
//! match the model does not compile:
//!
//! ```rust,compile_fail
//! use rango::prelude::*;
//!
//! #[derive(Model)]
//! struct User {
//!     id: FieldUuid,
//!     age: FieldInt,
//! }
//!
//! #[derive(Partial)]
//! #[partial(model = User)]
//! struct UserLite {
//!     age: FieldText,
//! }
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
pub use rango_orm_derive::Partial;
pub use rango_orm_derive::model;

// ─── Postgres backend (feature `postgres`, enabled by default) ───────────────

#[cfg(feature = "postgres")]
pub use rango_orm_postgres::PgPool;
#[cfg(feature = "postgres")]
pub use rango_orm_postgres::{
    Conn, M2M, Pg, PgQueryExt, QueryBuilder, RangoFilterExt, RangoTransaction, SelectAs,
    WithRelated, all, atomic, atomic_retry, bulk_create, bulk_update, bulk_upsert, delete, get,
    get_or_create, insert, raw, raw_execute, raw_scalar, transaction, update, update_or_create,
};

/// PostgreSQL ops, always reachable by this path whatever the other features.
#[cfg(feature = "postgres")]
pub mod postgres {
    pub use rango_orm_postgres::{
        Conn, M2M, Pg, PgPool, PgQueryExt, QueryBuilder, RangoFilterExt, RangoTransaction,
        SelectAs, WithRelated, all, atomic, atomic_retry, bulk_create, bulk_update, bulk_upsert,
        delete, get, get_or_create, insert, raw, raw_execute, raw_scalar, transaction, update,
        update_or_create,
    };
}

#[cfg(feature = "postgres")]
pub async fn connect_postgres(config: &DatabaseConfig) -> Result<PgPool> {
    rango_orm_postgres::connect(config).await
}

// ─── SQLite backend (feature `sqlite`) ────────────────────────────────────────

#[cfg(feature = "sqlite")]
pub use rango_orm_sqlite::SqlitePool;

/// SQLite ops, always reachable by this path whatever the other features.
#[cfg(feature = "sqlite")]
pub mod sqlite {
    pub use rango_orm_sqlite::{
        Conn, M2M, QueryBuilder, RangoFilterExt, SelectAs, SqlitePool, SqliteQueryExt,
        SqliteTransaction, WithRelated, all, atomic, bulk_create, bulk_update, bulk_upsert, delete,
        get, get_or_create, insert, raw, raw_execute, raw_scalar, transaction, update,
        update_or_create,
    };
}

#[cfg(feature = "sqlite")]
pub async fn connect_sqlite(url: &str) -> Result<SqlitePool> {
    rango_orm_sqlite::connect(url).await
}

// ─── MySQL / MariaDB backend (feature `mysql`) ───────────────────────────────

#[cfg(feature = "mysql")]
pub use rango_orm_mysql::MySqlPool;

/// MySQL 8.0+ / MariaDB 10.6+ ops, always reachable by this path whatever the other features.
#[cfg(feature = "mysql")]
pub mod mysql {
    pub use rango_orm_mysql::{
        Conn, M2M, MySqlPool, MySqlTransaction, QueryBuilder, RangoFilterExt, SelectAs,
        WithRelated, all, atomic, bulk_create, bulk_update, bulk_upsert, delete, get,
        get_or_create, insert, raw, raw_execute, raw_scalar, transaction, update, update_or_create,
    };
}

#[cfg(feature = "mysql")]
pub async fn connect_mysql(url: &str) -> Result<MySqlPool> {
    rango_orm_mysql::connect(url).await
}

// ─── Prelude ──────────────────────────────────────────────────────────────────

/// Backend-agnostic items, the connect functions and pool types of every enabled
/// backend, plus the ops (`insert`, `get`, `atomic`, `QueryBuilder`, ...) of exactly
/// ONE backend: postgres if enabled, else sqlite, else mysql. The other backends'
/// ops stay reachable as `rango::sqlite::*` / `rango::mysql::*` / `rango::postgres::*`.
pub mod prelude {
    #[cfg(feature = "mysql")]
    pub use crate::{MySqlPool, connect_mysql};
    #[cfg(feature = "postgres")]
    pub use crate::{PgPool, connect_postgres};
    #[cfg(feature = "sqlite")]
    pub use crate::{SqlitePool, connect_sqlite, sqlite::SqliteQueryExt};
    pub use rango_orm_core::ModelHooks;
    pub use rango_orm_core::*;
    pub use rango_orm_derive::{Model, ModelMixin, Partial};

    #[cfg(all(feature = "mysql", not(any(feature = "postgres", feature = "sqlite"))))]
    pub use crate::mysql::*;
    #[cfg(feature = "postgres")]
    pub use crate::postgres::*;
    #[cfg(all(feature = "sqlite", not(feature = "postgres")))]
    pub use crate::sqlite::*;
}
