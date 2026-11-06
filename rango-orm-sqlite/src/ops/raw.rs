//! Raw SQL escape hatch. The SQL string is trusted: never build it from user input.

use crate::error::db_err;
use crate::row::decode_all;
use rango_orm_core::Result;
use rango_orm_core::{FromRow, SqlValue};
use sqlx::{Executor, Sqlite};

use super::*;

/// Raw SQL — returns deserialized model rows.
pub async fn raw<'e, E, M>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<Vec<M>>
where
    E: Executor<'e, Database = Sqlite>,
    M: FromRow,
{
    let q = bind_sql_values(sqlx::query(sql), params);
    let rows = q.fetch_all(executor).await.map_err(db_err)?;
    decode_all::<M>(rows)
}

/// Raw SQL — returns a single scalar value.
pub async fn raw_scalar<'e, E, T>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<T>
where
    E: Executor<'e, Database = Sqlite>,
    T: for<'r> sqlx::Decode<'r, Sqlite> + sqlx::Type<Sqlite>,
{
    use sqlx::Row;
    let q = bind_sql_values(sqlx::query(sql), params);
    let row = q.fetch_one(executor).await.map_err(db_err)?;
    row.try_get::<T, _>(0).map_err(db_err)
}

/// Raw SQL — execute statement, return rows affected.
pub async fn raw_execute<'e, E>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<u64>
where
    E: Executor<'e, Database = Sqlite>,
{
    let q = bind_sql_values(sqlx::query(sql), params);
    q.execute(executor)
        .await
        .map(|r| r.rows_affected())
        .map_err(db_err)
}
