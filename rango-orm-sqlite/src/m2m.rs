use crate::error::db_err;
use rango_orm_core::related::M2mSql;
use rango_orm_core::{Dialect, Result};
use rango_orm_core::{FromRow, Model, ModelValues};

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
    fn sql() -> M2mSql {
        M2mSql::new(Dialect::Sqlite, From::table_name(), To::table_name())
    }

    fn pivot_table() -> String {
        Self::sql().pivot_table().to_owned()
    }

    /// Add a relation between from and to.
    /// Idempotent: an existing relation is left as is.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "INSERT", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn add<'e, E>(executor: E, from: &From, to: &To) -> Result<()>
    where
        E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
    {
        let sql = Self::sql().insert();
        let q = bind_sql_values(
            sqlx::query(sqlx::AssertSqlSafe(&*sql)),
            vec![from.pk_value(), to.pk_value()],
        );
        q.execute(executor).await.map_err(db_err)?;
        Ok(())
    }

    /// Remove a relation.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "DELETE", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn remove<'e, E>(executor: E, from: &From, to: &To) -> Result<()>
    where
        E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
    {
        let sql = Self::sql().remove();
        let q = bind_sql_values(
            sqlx::query(sqlx::AssertSqlSafe(&*sql)),
            vec![from.pk_value(), to.pk_value()],
        );
        q.execute(executor).await.map_err(db_err)?;
        Ok(())
    }

    /// Clear all relations from a given model instance.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "DELETE", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn clear<'e, E>(executor: E, from: &From) -> Result<()>
    where
        E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
    {
        let sql = Self::sql().clear();
        let q = bind_sql_values(
            sqlx::query(sqlx::AssertSqlSafe(&*sql)),
            vec![from.pk_value()],
        );
        q.execute(executor).await.map_err(db_err)?;
        Ok(())
    }

    /// Get all related `To` models for a given `from` instance.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn all<'e, E>(executor: E, from: &From) -> Result<Vec<To>>
    where
        E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
    {
        let sql = Self::sql().all(To::pk_column());
        let q = bind_sql_values(
            sqlx::query(sqlx::AssertSqlSafe(&*sql)),
            vec![from.pk_value()],
        );
        let rows = q.fetch_all(executor).await.map_err(db_err)?;

        decode_all::<To>(rows)
    }

    /// Get all `From` models related to a given `to` instance (reverse).
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn reverse<'e, E>(executor: E, to: &To) -> Result<Vec<From>>
    where
        E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
        From: FromRow,
    {
        let sql = Self::sql().reverse(From::pk_column());
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), vec![to.pk_value()]);
        let rows = q.fetch_all(executor).await.map_err(db_err)?;

        decode_all::<From>(rows)
    }

    /// Set exact relations — replaces all existing with the given list, atomically.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "UPDATE", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn set<'a, A>(executor: A, from: &From, to_list: &[To]) -> Result<()>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Sqlite>,
    {
        let mut tx = executor.begin().await.map_err(db_err)?;
        let s = Self::sql();
        let sql = s.clear();
        let q = bind_sql_values(
            sqlx::query(sqlx::AssertSqlSafe(&*sql)),
            vec![from.pk_value()],
        );
        q.execute(&mut *tx).await.map_err(db_err)?;
        let sql = s.insert();
        for to in to_list {
            let q = bind_sql_values(
                sqlx::query(sqlx::AssertSqlSafe(&*sql)),
                vec![from.pk_value(), to.pk_value()],
            );
            q.execute(&mut *tx).await.map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }

    /// Check if a relation exists.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn exists<'e, E>(executor: E, from: &From, to: &To) -> Result<bool>
    where
        E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
    {
        let sql = Self::sql().exists();
        let q = bind_sql_values(
            sqlx::query(sqlx::AssertSqlSafe(&*sql)),
            vec![from.pk_value(), to.pk_value()],
        );
        let row = q.fetch_optional(executor).await.map_err(db_err)?;
        Ok(row.is_some())
    }
}
