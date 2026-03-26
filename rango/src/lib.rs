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
    all, atomic, transaction, delete, get, get_or_create,
    insert, update, update_or_create,
    raw, raw_scalar, raw_execute,
    M2M, QueryBuilder, RangoFilterExt, WithRelated,
    RangoExecutor, RangoTransaction,
    PgQueryExt, Pg,
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
        all, delete, get, get_or_create, insert, update, bulk_create,
        raw, raw_scalar, raw_execute,
        QueryBuilder, RangoFilterExt,
        atomic, SqliteTransaction,
    };
}

#[cfg(feature = "sqlite")]
pub async fn connect_sqlite(url: &str) -> anyhow::Result<SqlitePool> {
    rango_sqlite::connect(url).await
}

// ─── Prelude ──────────────────────────────────────────────────────────────────

pub mod prelude {
    pub use rango_core::*;
    pub use rango_derive::{Model, ModelMixin};
    pub use rango_core::ModelHooks;

    pub use rango_postgres::{
        all, atomic, transaction, delete, get, get_or_create,
        insert, update, update_or_create,
        raw, raw_scalar, raw_execute,
        PgPool, QueryBuilder, RangoFilterExt, WithRelated, M2M,
        RangoExecutor, RangoTransaction,
        PgQueryExt, Pg,
    };

    #[cfg(feature = "sqlite")]
    pub use rango_sqlite::SqlitePool;
    #[cfg(feature = "sqlite")]
    pub use crate::sqlite::*;
}
