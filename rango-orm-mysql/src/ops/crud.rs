//! Single-row CRUD operations.

use crate::error::db_err;
use crate::row::{decode_all, MySqlRangoRow};
use rango_orm_core::{
    after_delete, after_insert, after_update, before_delete, before_insert, before_update,
};
use rango_orm_core::{quote_ident_mysql as quote_ident, FromRow, Model, ModelValues, SqlValue};
use rango_orm_core::{Error, Result};
use sqlx::{Acquire, Executor, MySql};

use super::*;

// ─── CRUD ops ─────────────────────────────────────────────────────────────────
// MySQL has no RETURNING. Every column is bound from the model (no DB-side defaults on insert
// except auto-increment PKs, read back with LAST_INSERT_ID()), so the input model is what was
// stored, minus DATETIME(6) sub-µs precision.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "INSERT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn insert<'e, E, M>(executor: E, mut model: M) -> Result<M>
where
    E: Executor<'e, Database = MySql>,
    M: Model + ModelValues + FromRow,
{
    before_insert(&mut model).await?;
    let generated = model.pk_value().is_null();
    let (sql, values) = build_insert_sql::<M>(&model);
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), values);
    let res = q.execute(executor).await.map_err(db_err)?;
    if generated {
        model.set_generated_pk(res.last_insert_id() as i64);
    }
    after_insert(&model).await?;
    Ok(model)
}

/// Fails with [`Error::NotFound`] when no row has the model's PK
/// (sqlx sets `CLIENT_FOUND_ROWS`, so unchanged rows still count as matched).
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "UPDATE", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn update<'e, E, M>(executor: E, mut model: M) -> Result<M>
where
    E: Executor<'e, Database = MySql>,
    M: Model + ModelValues + FromRow,
{
    before_update(&mut model).await?;
    let (sql, values) = build_update_sql::<M>(&model, model.pk_value());
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), values);
    let res = q.execute(executor).await.map_err(db_err)?;
    if res.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    after_update(&model).await?;
    Ok(model)
}

#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "DELETE", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn delete<'e, E, M>(executor: E, model: &M) -> Result<()>
where
    E: Executor<'e, Database = MySql>,
    M: Model + ModelValues,
{
    before_delete(model).await?;
    let sql = format!(
        "DELETE FROM {} WHERE {} = ?",
        quote_ident(M::table_name()),
        quote_ident(M::pk_column())
    );
    let q = bind_sql_values(
        sqlx::query(sqlx::AssertSqlSafe(&*sql)),
        vec![model.pk_value()],
    );
    q.execute(executor).await.map_err(db_err)?;
    after_delete(model).await
}

#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn get<'e, E, M>(executor: E, pk: &SqlValue) -> Result<Option<M>>
where
    E: Executor<'e, Database = MySql>,
    M: Model + ModelValues + FromRow,
{
    let sql = format!(
        "SELECT * FROM {} WHERE {} = ? LIMIT 1",
        quote_ident(M::table_name()),
        quote_ident(M::pk_column())
    );
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), vec![pk.clone()]);
    let row = q.fetch_optional(executor).await.map_err(db_err)?;
    match row {
        Some(r) => Ok(Some(M::from_row(&MySqlRangoRow(r)).map_err(Error::from)?)),
        None => Ok(None),
    }
}

/// All rows of the table. Uncapped: unlike `QueryBuilder::all` (1000-row default cap), this returns every row.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn all<'e, E, M>(executor: E) -> Result<Vec<M>>
where
    E: Executor<'e, Database = MySql>,
    M: Model + ModelValues + FromRow,
{
    let sql = format!("SELECT * FROM {}", quote_ident(M::table_name()));
    let rows = sqlx::query(sqlx::AssertSqlSafe(&*sql))
        .fetch_all(executor)
        .await
        .map_err(db_err)?;
    decode_all::<M>(rows)
}

/// Plain INSERT; on a unique violation, fetch the existing row. (`INSERT IGNORE` would also
/// swallow NOT NULL / FK / truncation errors.)
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "INSERT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn get_or_create<'a, A, M>(
    executor: A,
    lookup: Vec<(&'static str, SqlValue)>,
    mut defaults: M,
) -> Result<(M, bool)>
where
    A: Acquire<'a, Database = MySql>,
    M: Model + ModelValues + FromRow,
{
    let mut conn = executor.acquire().await.map_err(db_err)?;
    before_insert(&mut defaults).await?;
    let generated = defaults.pk_value().is_null();
    let (insert_sql, insert_vals) = build_insert_sql::<M>(&defaults);
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*insert_sql)), insert_vals);
    match q.execute(&mut *conn).await.map_err(db_err) {
        Ok(res) => {
            if generated {
                defaults.set_generated_pk(res.last_insert_id() as i64);
            }
            after_insert(&defaults).await?;
            return Ok((defaults, true));
        }
        Err(Error::UniqueViolation { .. }) => {}
        Err(e) => return Err(e),
    }

    let (select_sql, select_vals) = build_lookup_sql::<M>(&lookup);
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*select_sql)), select_vals);
    let row = q.fetch_one(&mut *conn).await.map_err(db_err)?;
    Ok((
        M::from_row(&MySqlRangoRow(row)).map_err(Error::from)?,
        false,
    ))
}

/// UPDATE OR CREATE.
///
/// Updates the row matching `lookup` (update hooks fire), or inserts `values` (create hooks fire).
/// Accepts `&MySqlPool`, `&mut *tx` or any type implementing `sqlx::Acquire`.
/// Lookup then write are two statements: wrap in `atomic()` to make them one unit.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "UPDATE", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn update_or_create<'a, A, M>(
    executor: A,
    lookup: Vec<(&'static str, SqlValue)>,
    mut values: M,
) -> Result<M>
where
    A: Acquire<'a, Database = MySql>,
    M: Model + ModelValues + FromRow,
{
    let mut conn = executor.acquire().await.map_err(db_err)?;
    let (lookup_sql, lookup_vals) = build_lookup_sql::<M>(&lookup);
    let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*lookup_sql)), lookup_vals);
    let row = q.fetch_optional(&mut *conn).await.map_err(db_err)?;
    if let Some(row) = row {
        // Update the row that exists (its PK), not the PK carried by `values`.
        let existing = M::from_row(&MySqlRangoRow(row)).map_err(Error::from)?;
        before_update(&mut values).await?;
        let (update_sql, update_vals) = build_update_sql::<M>(&values, existing.pk_value());
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*update_sql)), update_vals);
        q.execute(&mut *conn).await.map_err(db_err)?;
        // No RETURNING: read the stored row back by PK.
        let (sel_sql, sel_vals) = build_lookup_sql::<M>(&[(M::pk_column(), existing.pk_value())]);
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sel_sql)), sel_vals);
        let row = q.fetch_one(&mut *conn).await.map_err(db_err)?;
        let saved = M::from_row(&MySqlRangoRow(row)).map_err(Error::from)?;
        after_update(&saved).await?;
        Ok(saved)
    } else {
        before_insert(&mut values).await?;
        let generated = values.pk_value().is_null();
        let (insert_sql, insert_vals) = build_insert_sql::<M>(&values);
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*insert_sql)), insert_vals);
        let res = q.execute(&mut *conn).await.map_err(db_err)?;
        if generated {
            values.set_generated_pk(res.last_insert_id() as i64);
        }
        after_insert(&values).await?;
        Ok(values)
    }
}

// ─── SQL builders ─────────────────────────────────────────────────────────────
fn build_insert_sql<M: Model + ModelValues>(model: &M) -> (String, Vec<SqlValue>) {
    let (cols, values): (Vec<_>, Vec<_>) = rango_orm_core::insert_values(model).into_iter().unzip();
    let cols: Vec<String> = cols.into_iter().map(quote_ident).collect();
    let placeholders: Vec<&str> = vec!["?"; cols.len()];
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
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
        .map(|(col, _)| format!("{} = ?", quote_ident(col)))
        .collect();
    let sql = format!(
        "UPDATE {} SET {} WHERE {} = ?",
        quote_ident(M::table_name()),
        set_clauses.join(", "),
        quote_ident(pk_col)
    );
    let mut values: Vec<SqlValue> = fields.into_iter().map(|(_, v)| v).collect();
    values.push(pk_val);
    (sql, values)
}

fn build_lookup_sql<M: Model + ModelValues>(
    lookup: &[(&'static str, SqlValue)],
) -> (String, Vec<SqlValue>) {
    let conditions: Vec<String> = lookup
        .iter()
        .map(|(col, _)| format!("{} = ?", quote_ident(col)))
        .collect();
    let sql = format!(
        "SELECT * FROM {} WHERE {} LIMIT 1",
        quote_ident(M::table_name()),
        conditions.join(" AND ")
    );
    let values = lookup.iter().map(|(_, v)| v.clone()).collect();
    (sql, values)
}
