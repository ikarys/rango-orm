/// Rango ORM — re-exports everything the user needs.
///
/// # Quick start
///
/// Add to `Cargo.toml`:
/// ```toml
/// rango = { version = "0.1", features = ["sqlite"] }   # default
/// # or
/// rango = { version = "0.1", features = ["postgres"] }
/// ```
///
/// Then:
/// ```rust,ignore
/// use rango::prelude::*;
///
/// #[derive(Model)]
/// struct User {
///     id: FieldUuid,
///     email: FieldEmail,
/// }
/// ```
// Re-export core types — always available
pub use rango_core::*;

// Make rango_core visible for proc-macro generated code.
#[doc(hidden)]
pub extern crate rango_core;

// Re-export derive macros
pub use rango_derive::Model;
pub use rango_derive::ModelMixin;

// ─── Postgres backend (always available) ─────────────────────────────────────

pub use rango_postgres::PgPool;
pub use rango_postgres::{
    M2M, Pg, PgQueryExt, QueryBuilder, RangoExecutor, RangoFilterExt, RangoTransaction,
    WithRelated, all, atomic, delete, get, get_or_create, insert, raw, raw_execute, raw_scalar,
    transaction, update, update_or_create,
};

pub async fn connect_postgres(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    rango_postgres::connect(config).await
}

// ─── SQLite backend (optional, enabled by default) ───────────────────────────

#[cfg(feature = "sqlite")]
pub use rango_sqlite::SqlitePool;

/// SQLite-specific ops — use these when working with a SqlitePool.
/// Parallel to the postgres ops but for SQLite.
#[cfg(feature = "sqlite")]
pub mod sqlite {
    pub use rango_sqlite::{
        QueryBuilder, RangoFilterExt, SqliteTransaction, all, atomic, bulk_create, delete, get,
        get_or_create, insert, raw, raw_execute, raw_scalar, update,
    };
}

#[cfg(feature = "sqlite")]
pub async fn connect_sqlite(url: &str) -> anyhow::Result<SqlitePool> {
    rango_sqlite::connect(url).await
}

// ─── Prelude ──────────────────────────────────────────────────────────────────

pub mod prelude {
    pub use rango_core::ModelHooks;
    pub use rango_core::*;
    pub use rango_derive::{Model, ModelMixin};

    pub use rango_postgres::{
        M2M, Pg, PgPool, PgQueryExt, QueryBuilder, RangoExecutor, RangoFilterExt, RangoTransaction,
        WithRelated, all, atomic, delete, get, get_or_create, insert, raw, raw_execute, raw_scalar,
        transaction, update, update_or_create,
    };

    #[cfg(feature = "sqlite")]
    pub use crate::sqlite::*;
    #[cfg(feature = "sqlite")]
    pub use rango_sqlite::SqlitePool;
}
