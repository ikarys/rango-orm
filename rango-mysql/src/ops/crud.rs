//! Single-row CRUD operations.

use crate::error::db_err;
use crate::row::{decode_all, MySqlRangoRow};
use rango_core::{quote_ident_mysql as quote_ident, FromRow, Model, ModelValues, SqlValue};
use rango_core::{Error, Result};
use sqlx::{Executor, MySql, MySqlPool};

use super::*;

// ─── CRUD ops ─────────────────────────────────────────────────────────────────
// MySQL has no RETURNING. Every column is bound from the model (client-side PKs, no DB-side
// defaults on insert), so the input model is what was stored, minus DATETIME(6) sub-µs precision.
pub async fn insert<'e, E, M>(executor: E, model: M) -> Result<M>
where
    E: Executor<'e, Database = MySql>,
    M: Model + ModelValues + FromRow,
{
    let (sql, values) = build_insert_sql::<M>(&model);
    let q = bind_sql_values(sqlx::query(&sql), values);
    q.execute(executor).await.map_err(db_err)?;
    Ok(model)
}

/// Fails with [`Error::NotFound`] when no row has the model's PK
/// (sqlx sets `CLIENT_FOUND_ROWS`, so unchanged rows still count as matched).
pub async fn update<'e, E, M>(executor: E, model: M) -> Result<M>
where
    E: Executor<'e, Database = MySql>,
    M: Model + ModelValues + FromRow,
{
    let (sql, values) = build_update_sql::<M>(&model);
    let q = bind_sql_values(sqlx::query(&sql), values);
    let res = q.execute(executor).await.map_err(db_err)?;
    if res.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    Ok(model)
}

pub async fn delete<'e, E, M>(executor: E, model: &M) -> Result<()>
where
    E: Executor<'e, Database = MySql>,
    M: Model + ModelValues,
{
    let sql = format!(
        "DELETE FROM {} WHERE {} = ?",
        quote_ident(M::table_name()),
        quote_ident(M::pk_column())
    );
    let q = bind_sql_values(sqlx::query(&sql), vec![model.pk_value()]);
    q.execute(executor).await.map(|_| ()).map_err(db_err)
}

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
    let q = bind_sql_values(sqlx::query(&sql), vec![pk.clone()]);
    let row = q.fetch_optional(executor).await.map_err(db_err)?;
    match row {
        Some(r) => Ok(Some(M::from_row(&MySqlRangoRow(r)).map_err(Error::from)?)),
        None => Ok(None),
    }
}

pub async fn all<'e, E, M>(executor: E) -> Result<Vec<M>>
where
    E: Executor<'e, Database = MySql>,
    M: Model + ModelValues + FromRow,
{
    let sql = format!("SELECT * FROM {}", quote_ident(M::table_name()));
    let rows = sqlx::query(&sql)
        .fetch_all(executor)
        .await
        .map_err(db_err)?;
    decode_all::<M>(rows)
}

/// Plain INSERT; on a unique violation, fetch the existing row. (`INSERT IGNORE` would also
/// swallow NOT NULL / FK / truncation errors.)
pub async fn get_or_create<M>(
    pool: &MySqlPool,
    lookup: Vec<(&'static str, SqlValue)>,
    defaults: M,
) -> Result<(M, bool)>
where
    M: Model + ModelValues + FromRow,
{
    let (insert_sql, insert_vals) = build_insert_sql::<M>(&defaults);
    let q = bind_sql_values(sqlx::query(&insert_sql), insert_vals);
    match q.execute(pool).await.map_err(db_err) {
        Ok(_) => return Ok((defaults, true)),
        Err(Error::UniqueViolation { .. }) => {}
        Err(e) => return Err(e),
    }

    let (select_sql, select_vals) = build_lookup_sql::<M>(&lookup);
    let q = bind_sql_values(sqlx::query(&select_sql), select_vals);
    let row = q.fetch_one(pool).await.map_err(db_err)?;
    Ok((
        M::from_row(&MySqlRangoRow(row)).map_err(Error::from)?,
        false,
    ))
}

// ─── SQL builders ─────────────────────────────────────────────────────────────
fn build_insert_sql<M: Model + ModelValues>(model: &M) -> (String, Vec<SqlValue>) {
    let fields = model.field_values();
    let pk_col = M::pk_column();
    let pk_val = model.pk_value();
    let mut cols = vec![quote_ident(pk_col)];
    cols.extend(fields.iter().map(|(c, _)| quote_ident(c)));
    let placeholders: Vec<String> = (0..cols.len()).map(|_| "?".to_string()).collect();
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        quote_ident(M::table_name()),
        cols.join(", "),
        placeholders.join(", ")
    );
    let mut values = vec![pk_val];
    values.extend(fields.into_iter().map(|(_, v)| v));
    (sql, values)
}

fn build_update_sql<M: Model + ModelValues>(model: &M) -> (String, Vec<SqlValue>) {
    let fields = model.field_values();
    let pk_col = M::pk_column();
    let pk_val = model.pk_value();
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
