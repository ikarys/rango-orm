//! Single-row CRUD operations.

use crate::error::db_err;
use crate::row::{PgRangoRow, decode_all};
use rango_orm_core::{Error, Result};
use rango_orm_core::{FromRow, Model, ModelValues, SqlValue, quote_ident};
use rango_orm_core::{
    after_delete, after_insert, after_update, before_delete, before_insert, before_update,
};
use sqlx::{Acquire, Executor, Postgres};

use super::*;

// ─── Single-query ops — transparent over PgPool, Transaction, and any connection ─
/// INSERT → returns the model as stored.
/// Accepts `&PgPool`, `&mut Transaction<'_, Postgres>`, or any sqlx `Executor`.
///
/// # Example
/// ```rust,ignore
/// // With pool
/// let user = rango::insert(&pool, user).await?;
///
/// // Inside rango::atomic()
/// let user = rango::atomic(&pool, async |tx| {
///     let user = rango::insert(&mut **tx, user).await?;
///     rango::insert(&mut **tx, profile).await?;
///     Ok(user)
/// }).await?;
/// ```
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "INSERT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn insert<'e, E, M>(executor: E, mut model: M) -> Result<M>
where
    E: Executor<'e, Database = Postgres>,
    M: Model + ModelValues + FromRow,
{
    before_insert(&mut model).await?;
    let (sql, values) = build_insert_sql::<M>(&model);
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), values);
    let row = q.fetch_one(executor).await.map_err(db_err)?;
    let saved = M::from_row(&PgRangoRow(row)).map_err(Error::from)?;
    after_insert(&saved).await?;
    Ok(saved)
}

/// UPDATE → returns the updated model.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "UPDATE", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn update<'e, E, M>(executor: E, mut model: M) -> Result<M>
where
    E: Executor<'e, Database = Postgres>,
    M: Model + ModelValues + FromRow,
{
    before_update(&mut model).await?;
    let (sql, values) = build_update_sql::<M>(&model, model.pk_value());
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), values);
    let row = q.fetch_one(executor).await.map_err(db_err)?;
    let saved = M::from_row(&PgRangoRow(row)).map_err(Error::from)?;
    after_update(&saved).await?;
    Ok(saved)
}

/// DELETE a model.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "DELETE", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn delete<'e, E, M>(executor: E, model: &M) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
    M: Model + ModelValues,
{
    before_delete(model).await?;
    let (sql, values) = build_delete_sql::<M>(model);
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), values);
    q.execute(executor).await.map_err(db_err)?;
    after_delete(model).await
}

/// GET by primary key.
/// Accepts `&PgPool`, `&mut Transaction<'_, Postgres>`, or any sqlx `Executor`.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn get<'e, E, M>(executor: E, pk: &SqlValue) -> Result<Option<M>>
where
    E: Executor<'e, Database = Postgres>,
    M: Model + ModelValues + FromRow,
{
    let sql = format!(
        "SELECT * FROM {} WHERE {} = $1 LIMIT 1",
        quote_ident(M::table_name()),
        quote_ident(M::pk_column())
    );
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), vec![pk.clone()]);
    let row = q.fetch_optional(executor).await.map_err(db_err)?;
    match row {
        Some(r) => Ok(Some(M::from_row(&PgRangoRow(r)).map_err(Error::from)?)),
        None => Ok(None),
    }
}

/// GET ALL rows. Uncapped: unlike `QueryBuilder::all` (1000-row default cap), this returns every row of the table.
/// Accepts `&PgPool`, `&mut Transaction<'_, Postgres>`, or any sqlx `Executor`.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn all<'e, E, M>(executor: E) -> Result<Vec<M>>
where
    E: Executor<'e, Database = Postgres>,
    M: Model + ModelValues + FromRow,
{
    let sql = format!("SELECT * FROM {}", quote_ident(M::table_name()));
    let rows = sqlx::query(sqlx::AssertSqlSafe(&*sql))
        .fetch_all(executor)
        .await
        .map_err(db_err)?;
    decode_all::<M>(rows)
}

// ─── Compound ops — pool or transaction, sequential queries on one connection ─
/// GET OR CREATE — atomic, race-free. Returns `(model, created: bool)`.
///
/// Uses `INSERT ... ON CONFLICT (lookup_cols) DO NOTHING RETURNING *` in a single
/// round-trip. If the row already exists (conflict), falls back to a SELECT on the
/// same connection. No need to wrap in `atomic()`.
///
/// Accepts `&PgPool`, `&mut Transaction<'_, Postgres>`, or any type implementing
/// `sqlx::Acquire`.
///
/// # Example
/// ```rust,ignore
/// // Standalone — atomic without wrapping in rango::atomic()
/// let (user, created) = rango::get_or_create(&pool, lookup, defaults).await?;
///
/// // Inside a transaction
/// let (user, created) = rango::atomic(&pool, async |tx| {
///     rango::get_or_create(&mut **tx, lookup, defaults).await
/// }).await?;
/// ```
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "INSERT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn get_or_create<'a, A, M>(
    executor: A,
    lookup: Vec<(&'static str, SqlValue)>,
    mut defaults: M,
) -> Result<(M, bool)>
where
    A: Acquire<'a, Database = Postgres>,
    M: Model + ModelValues + FromRow,
{
    let mut conn = executor.acquire().await.map_err(db_err)?;

    before_insert(&mut defaults).await?;
    let conflict_cols: Vec<&str> = lookup.iter().map(|(col, _)| *col).collect();
    let (insert_sql, insert_vals) = build_get_or_create_sql(&defaults, &conflict_cols);
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*insert_sql)), insert_vals);
    let row = q.fetch_optional(&mut *conn).await.map_err(db_err)?;

    if let Some(r) = row {
        let created = M::from_row(&PgRangoRow(r)).map_err(Error::from)?;
        after_insert(&created).await?;
        return Ok((created, true));
    }

    // Conflict — row already exists; fetch it
    let (select_sql, select_vals) = build_lookup_sql::<M>(&lookup);
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*select_sql)), select_vals);
    let row = q.fetch_one(&mut *conn).await.map_err(db_err)?;

    Ok((M::from_row(&PgRangoRow(row)).map_err(Error::from)?, false))
}

/// UPDATE OR CREATE.
///
/// Updates the row matching `lookup` (update hooks fire), or inserts `values` (create hooks fire).
/// Accepts `&PgPool`, `&mut *tx` or any type implementing `sqlx::Acquire`.
/// Lookup then write are two statements: wrap in `atomic()` to make them one unit.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "UPDATE", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn update_or_create<'a, A, M>(
    executor: A,
    lookup: Vec<(&'static str, SqlValue)>,
    mut values: M,
) -> Result<M>
where
    A: Acquire<'a, Database = Postgres>,
    M: Model + ModelValues + FromRow,
{
    let mut conn = executor.acquire().await.map_err(db_err)?;
    let (lookup_sql, lookup_vals) = build_lookup_sql::<M>(&lookup);
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*lookup_sql)), lookup_vals);
    let row = q.fetch_optional(&mut *conn).await.map_err(db_err)?;
    if let Some(row) = row {
        // Update the row that exists (its PK), not the PK carried by `values`.
        let existing = M::from_row(&PgRangoRow(row)).map_err(Error::from)?;
        before_update(&mut values).await?;
        let (update_sql, update_vals) = build_update_sql::<M>(&values, existing.pk_value());
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*update_sql)), update_vals);
        let row = q.fetch_one(&mut *conn).await.map_err(db_err)?;
        let saved = M::from_row(&PgRangoRow(row)).map_err(Error::from)?;
        after_update(&saved).await?;
        Ok(saved)
    } else {
        before_insert(&mut values).await?;
        let (insert_sql, insert_vals) = build_insert_sql::<M>(&values);
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*insert_sql)), insert_vals);
        let row = q.fetch_one(&mut *conn).await.map_err(db_err)?;
        let saved = M::from_row(&PgRangoRow(row)).map_err(Error::from)?;
        after_insert(&saved).await?;
        Ok(saved)
    }
}

// ─── SQL builders ─────────────────────────────────────────────────────────────
fn build_insert_sql<M: Model + ModelValues>(model: &M) -> (String, Vec<SqlValue>) {
    let (cols, values): (Vec<_>, Vec<_>) = rango_orm_core::insert_values(model).into_iter().unzip();
    let cols: Vec<String> = cols.into_iter().map(quote_ident).collect();
    let placeholders: Vec<String> = (1..=cols.len()).map(|i| format!("${}", i)).collect();
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({}) RETURNING *",
        quote_ident(M::table_name()),
        cols.join(", "),
        placeholders.join(", ")
    );
    (sql, values)
}

fn build_update_sql<M: Model + ModelValues>(
    model: &M,
    pk_val: SqlValue,
) -> (String, Vec<SqlValue>) {
    let fields = model.field_values();
    let pk_col = M::pk_column();
    let set_clauses: Vec<String> = fields
        .iter()
        .enumerate()
        .map(|(i, (col, _))| format!("{} = ${}", quote_ident(col), i + 1))
        .collect();
    let sql = format!(
        "UPDATE {} SET {} WHERE {} = ${} RETURNING *",
        quote_ident(M::table_name()),
        set_clauses.join(", "),
        quote_ident(pk_col),
        fields.len() + 1
    );
    let mut values: Vec<SqlValue> = fields.into_iter().map(|(_, v)| v).collect();
    values.push(pk_val);
    (sql, values)
}

fn build_delete_sql<M: Model + ModelValues>(model: &M) -> (String, Vec<SqlValue>) {
    (
        format!(
            "DELETE FROM {} WHERE {} = $1",
            quote_ident(M::table_name()),
            quote_ident(M::pk_column())
        ),
        vec![model.pk_value()],
    )
}

fn build_get_or_create_sql<M: Model + ModelValues>(
    model: &M,
    conflict_cols: &[&str],
) -> (String, Vec<SqlValue>) {
    let (cols, values): (Vec<_>, Vec<_>) = rango_orm_core::insert_values(model).into_iter().unzip();
    let cols: Vec<String> = cols.into_iter().map(quote_ident).collect();
    let placeholders: Vec<String> = (1..=cols.len()).map(|i| format!("${}", i)).collect();
    let conflict_clause = conflict_cols
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) DO NOTHING RETURNING *",
        quote_ident(M::table_name()),
        cols.join(", "),
        placeholders.join(", "),
        conflict_clause
    );
    (sql, values)
}

fn build_lookup_sql<M: Model + ModelValues>(
    lookup: &[(&'static str, SqlValue)],
) -> (String, Vec<SqlValue>) {
    let conditions: Vec<String> = lookup
        .iter()
        .enumerate()
        .map(|(i, (col, _))| format!("{} = ${}", quote_ident(col), i + 1))
        .collect();
    let sql = format!(
        "SELECT * FROM {} WHERE {} LIMIT 1",
        quote_ident(M::table_name()),
        conditions.join(" AND ")
    );
    let values = lookup.iter().map(|(_, v)| v.clone()).collect();
    (sql, values)
}
