//! Raw SQL escape hatch. The SQL string is trusted: never build it from user input.

use crate::error::db_err;
use crate::row::decode_all;
use rango_orm_core::Result;
use rango_orm_core::{FromRow, SqlValue};
use sqlx::{Executor, Postgres};

use super::*;

// ─── Raw SQL escape hatch ─────────────────────────────────────────────────────
/// Execute a raw SQL query and return deserialized rows.
///
/// Parameters are positional (`$1`, `$2`, ...) as in PostgreSQL.
///
/// # Example
/// ```rust,ignore
/// let rows = rango::raw::<User>(&pool, "SELECT * FROM users WHERE role = $1", vec![SqlValue::Text("admin".into())]).await?;
/// ```
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn raw<'e, E, M>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<Vec<M>>
where
    E: Executor<'e, Database = Postgres>,
    M: FromRow,
{
    let q = bind_sql_values(sqlx::query(sql), params);
    let rows = q.fetch_all(executor).await.map_err(db_err)?;
    tracing::Span::current().record("db.rows", rows.len());
    decode_all::<M>(rows)
}

/// Execute a raw SQL query and return a single scalar value (first column of first row).
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn raw_scalar<'e, E, T>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<T>
where
    E: Executor<'e, Database = Postgres>,
    T: for<'r> sqlx::Decode<'r, Postgres> + sqlx::Type<Postgres>,
{
    use sqlx::Row;
    let q = bind_sql_values(sqlx::query(sql), params);
    let row = q.fetch_one(executor).await.map_err(db_err)?;
    row.try_get::<T, _>(0).map_err(db_err)
}

/// Execute a raw SQL statement (INSERT/UPDATE/DELETE/DDL) — returns rows affected.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "EXECUTE", db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn raw_execute<'e, E>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<u64>
where
    E: Executor<'e, Database = Postgres>,
{
    let q = bind_sql_values(sqlx::query(sql), params);
    q.execute(executor)
        .await
        .map(|r| r.rows_affected())
        .map_err(db_err)
}
