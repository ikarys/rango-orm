use crate::error::db_err;
use rango_orm_core::related::M2mSql;
use rango_orm_core::{Dialect, Result};
use rango_orm_core::{FromRow, Model, ModelValues};

use crate::ops::bind_sql_values;
use crate::row::decode_all;

/// Pairs per `INSERT` of `add_many` / `set` (two binds each, far under every bind limit).
const PAIRS_PER_STATEMENT: usize = 1000;

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
        M2mSql::new(Dialect::Mysql, From::table_name(), To::table_name())
    }

    fn pivot_table() -> String {
        Self::sql().pivot_table().to_owned()
    }

    /// Add a relation between from and to.
    /// Idempotent: an existing relation is left as is.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "INSERT", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn add<'e, E>(executor: E, from: &From, to: &To) -> Result<()>
    where
        E: sqlx::Executor<'e, Database = sqlx::MySql>,
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
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "DELETE", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn remove<'e, E>(executor: E, from: &From, to: &To) -> Result<()>
    where
        E: sqlx::Executor<'e, Database = sqlx::MySql>,
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
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "DELETE", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn clear<'e, E>(executor: E, from: &From) -> Result<()>
    where
        E: sqlx::Executor<'e, Database = sqlx::MySql>,
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
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "SELECT", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn all<'e, E>(executor: E, from: &From) -> Result<Vec<To>>
    where
        E: sqlx::Executor<'e, Database = sqlx::MySql>,
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
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "SELECT", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn reverse<'e, E>(executor: E, to: &To) -> Result<Vec<From>>
    where
        E: sqlx::Executor<'e, Database = sqlx::MySql>,
        From: FromRow,
    {
        let sql = Self::sql().reverse(From::pk_column());
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), vec![to.pk_value()]);
        let rows = q.fetch_all(executor).await.map_err(db_err)?;

        decode_all::<From>(rows)
    }

    /// Set exact relations — replaces all existing with the given list, atomically.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "UPDATE", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn set<'a, A>(executor: A, from: &From, to_list: &[To]) -> Result<()>
    where
        A: sqlx::Acquire<'a, Database = sqlx::MySql>,
    {
        let mut tx = executor.begin().await.map_err(db_err)?;
        let sql = Self::sql().clear();
        let q = bind_sql_values(
            sqlx::query(sqlx::AssertSqlSafe(&*sql)),
            vec![from.pk_value()],
        );
        q.execute(&mut *tx).await.map_err(db_err)?;
        Self::insert_pairs(&mut tx, from, to_list).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }

    /// Link `from` to every model of `to_list`: multi-row `INSERT`s of
    /// [`PAIRS_PER_STATEMENT`] pairs, in one transaction. Idempotent like `add`
    /// (existing links and duplicates in the list are left as is); an empty list runs no SQL.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "INSERT", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn add_many<'a, A>(executor: A, from: &From, to_list: &[To]) -> Result<()>
    where
        A: sqlx::Acquire<'a, Database = sqlx::MySql>,
    {
        if to_list.is_empty() {
            return Ok(());
        }
        let mut tx = executor.begin().await.map_err(db_err)?;
        Self::insert_pairs(&mut tx, from, to_list).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }

    async fn insert_pairs(
        tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
        from: &From,
        to_list: &[To],
    ) -> Result<()> {
        let s = Self::sql();
        for chunk in to_list.chunks(PAIRS_PER_STATEMENT) {
            let sql = s.insert_many(chunk.len());
            let mut binds = Vec::with_capacity(chunk.len() * 2);
            for to in chunk {
                binds.push(from.pk_value());
                binds.push(to.pk_value());
            }
            bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), binds)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
        }
        Ok(())
    }

    /// Check if a relation exists.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "SELECT", db.sql.table = %Self::pivot_table(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn exists<'e, E>(executor: E, from: &From, to: &To) -> Result<bool>
    where
        E: sqlx::Executor<'e, Database = sqlx::MySql>,
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
