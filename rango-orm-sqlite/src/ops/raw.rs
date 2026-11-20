//! Raw SQL escape hatch. The SQL string is trusted: never build it from user input.

use crate::error::db_err;
use crate::row::decode_all;
use rango_orm_core::Result;
use rango_orm_core::{FromRow, SqlValue};
use sqlx::{Executor, Sqlite};

use super::*;
use crate::query_builder::exec::row_to_map;

/// Raw SQL — returns deserialized model rows.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn raw<'e, E, M>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<Vec<M>>
where
    E: Executor<'e, Database = Sqlite>,
    M: FromRow,
{
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(sql)), params);
    let rows = q.fetch_all(executor).await.map_err(db_err)?;
    tracing::Span::current().record("db.rows", rows.len());
    decode_all::<M>(rows)
}

/// Raw SQL — returns each row as a `column name -> SqlValue` map, for result sets that are
/// not a model. Values decode like `.values()`: integers as `BigInt`, floats as `Double`.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn raw_values<'e, E>(
    executor: E,
    sql: &str,
    params: Vec<SqlValue>,
) -> Result<Vec<std::collections::HashMap<String, SqlValue>>>
where
    E: Executor<'e, Database = Sqlite>,
{
    use sqlx::{Column, Row};
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(sql)), params);
    let rows = q.fetch_all(executor).await.map_err(db_err)?;
    tracing::Span::current().record("db.rows", rows.len());
    let keys: Vec<(&str, Option<u8>)> = rows
        .first()
        .map(|r| r.columns().iter().map(|c| (c.name(), None)).collect())
        .unwrap_or_default();
    rows.iter().map(|row| row_to_map(row, &keys)).collect()
}

/// Raw SQL — returns a single scalar value.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn raw_scalar<'e, E, T>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<T>
where
    E: Executor<'e, Database = Sqlite>,
    T: for<'r> sqlx::Decode<'r, Sqlite> + sqlx::Type<Sqlite>,
{
    use sqlx::Row;
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(sql)), params);
    let row = q.fetch_one(executor).await.map_err(db_err)?;
    row.try_get::<T, _>(0).map_err(db_err)
}

/// Raw SQL — execute statement, return rows affected.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "EXECUTE", db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn raw_execute<'e, E>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<u64>
where
    E: Executor<'e, Database = Sqlite>,
{
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(sql)), params);
    q.execute(executor)
        .await
        .map(|r| r.rows_affected())
        .map_err(db_err)
}
