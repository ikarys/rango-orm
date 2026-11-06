use crate::error::db_err;
use rango_orm_core::related::m2m_pivot;
use rango_orm_core::Result;
use rango_orm_core::{quote_ident_mysql as quote_ident, FromRow, Model, ModelValues};
use sqlx::MySqlPool;

use crate::ops::bind_sql_values;
use crate::row::decode_all;

/// Many-to-many operations.
/// `From` = the model that declares the M2M field
/// `To`   = the related model
pub struct M2M<From, To> {
    _from: std::marker::PhantomData<From>,
    _to: std::marker::PhantomData<To>,
}

impl<From, To> M2M<From, To>
where
    From: Model + ModelValues,
    To: Model + ModelValues + FromRow,
{
    fn pivot_table() -> String {
        m2m_pivot(From::table_name(), To::table_name()).0
    }

    fn from_col() -> String {
        m2m_pivot(From::table_name(), To::table_name()).1
    }

    fn to_col() -> String {
        m2m_pivot(From::table_name(), To::table_name()).2
    }

    fn insert_sql() -> String {
        let pivot = Self::pivot_table();
        let from_col = Self::from_col();
        let to_col = Self::to_col();
        // Use ON DUPLICATE KEY UPDATE, not INSERT IGNORE, to avoid silencing FK and other errors.
        format!(
            "INSERT INTO {} ({}, {}) VALUES (?, ?) ON DUPLICATE KEY UPDATE {} = {}",
            quote_ident(&pivot),
            quote_ident(&from_col),
            quote_ident(&to_col),
            quote_ident(&from_col),
            quote_ident(&from_col)
        )
    }

    fn clear_sql() -> String {
        let pivot = Self::pivot_table();
        let from_col = Self::from_col();
        format!(
            "DELETE FROM {} WHERE {} = ?",
            quote_ident(&pivot),
            quote_ident(&from_col)
        )
    }

    /// Add a relation between from and to.
    /// Idempotent: an existing relation is left as is.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "INSERT", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn add(pool: &MySqlPool, from: &From, to: &To) -> Result<()> {
        let sql = Self::insert_sql();
        let q = bind_sql_values(sqlx::query(&sql), vec![from.pk_value(), to.pk_value()]);
        q.execute(pool).await.map_err(db_err)?;
        Ok(())
    }

    /// Remove a relation.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "DELETE", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn remove(pool: &MySqlPool, from: &From, to: &To) -> Result<()> {
        let pivot = Self::pivot_table();
        let from_col = Self::from_col();
        let to_col = Self::to_col();
        let sql = format!(
            "DELETE FROM {} WHERE {} = ? AND {} = ?",
            quote_ident(&pivot),
            quote_ident(&from_col),
            quote_ident(&to_col)
        );
        let q = bind_sql_values(sqlx::query(&sql), vec![from.pk_value(), to.pk_value()]);
        q.execute(pool).await.map_err(db_err)?;
        Ok(())
    }

    /// Clear all relations from a given model instance.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "DELETE", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn clear(pool: &MySqlPool, from: &From) -> Result<()> {
        let sql = Self::clear_sql();
        let q = bind_sql_values(sqlx::query(&sql), vec![from.pk_value()]);
        q.execute(pool).await.map_err(db_err)?;
        Ok(())
    }

    /// Get all related `To` models for a given `from` instance.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "SELECT", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn all(pool: &MySqlPool, from: &From) -> Result<Vec<To>> {
        let pivot = Self::pivot_table();
        let from_col = Self::from_col();
        let to_table = To::table_name();
        let to_pk = To::pk_column();
        let to_col = Self::to_col();

        let sql = format!(
            r#"SELECT t.* FROM {to_table} t
               INNER JOIN {pivot} p ON t.{to_pk} = p.{to_col}
               WHERE p.{from_col} = ?"#,
            to_table = quote_ident(to_table),
            pivot = quote_ident(&pivot),
            to_pk = quote_ident(to_pk),
            to_col = quote_ident(&to_col),
            from_col = quote_ident(&from_col),
        );
        let q = bind_sql_values(sqlx::query(&sql), vec![from.pk_value()]);
        let rows = q.fetch_all(pool).await.map_err(db_err)?;

        decode_all::<To>(rows)
    }

    /// Get all `From` models related to a given `to` instance (reverse).
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "SELECT", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn reverse(pool: &MySqlPool, to: &To) -> Result<Vec<From>>
    where
        From: FromRow,
    {
        let pivot = Self::pivot_table();
        let to_col = Self::to_col();
        let from_table = From::table_name();
        let from_pk = From::pk_column();
        let from_col = Self::from_col();

        let sql = format!(
            r#"SELECT f.* FROM {from_table} f
               INNER JOIN {pivot} p ON f.{from_pk} = p.{from_col}
               WHERE p.{to_col} = ?"#,
            from_table = quote_ident(from_table),
            pivot = quote_ident(&pivot),
            from_pk = quote_ident(from_pk),
            from_col = quote_ident(&from_col),
            to_col = quote_ident(&to_col),
        );
        let q = bind_sql_values(sqlx::query(&sql), vec![to.pk_value()]);
        let rows = q.fetch_all(pool).await.map_err(db_err)?;

        decode_all::<From>(rows)
    }

    /// Set exact relations — replaces all existing with the given list, atomically.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "UPDATE", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn set(pool: &MySqlPool, from: &From, to_list: &[To]) -> Result<()> {
        let mut tx = pool.begin().await.map_err(db_err)?;
        let sql = Self::clear_sql();
        let q = bind_sql_values(sqlx::query(&sql), vec![from.pk_value()]);
        q.execute(&mut *tx).await.map_err(db_err)?;
        let sql = Self::insert_sql();
        for to in to_list {
            let q = bind_sql_values(sqlx::query(&sql), vec![from.pk_value(), to.pk_value()]);
            q.execute(&mut *tx).await.map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }

    /// Check if a relation exists.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "SELECT", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn exists(pool: &MySqlPool, from: &From, to: &To) -> Result<bool> {
        let pivot = Self::pivot_table();
        let from_col = Self::from_col();
        let to_col = Self::to_col();
        let sql = format!(
            "SELECT 1 FROM {} WHERE {} = ? AND {} = ? LIMIT 1",
            quote_ident(&pivot),
            quote_ident(&from_col),
            quote_ident(&to_col)
        );
        let q = bind_sql_values(sqlx::query(&sql), vec![from.pk_value(), to.pk_value()]);
        let row = q.fetch_optional(pool).await.map_err(db_err)?;
        Ok(row.is_some())
    }
}
