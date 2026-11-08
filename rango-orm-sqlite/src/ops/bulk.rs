//! Multi-row operations: bulk insert, update and upsert.

use crate::error::db_err;
use rango_orm_core::{quote_ident, Model, ModelValues, SqlValue};
use rango_orm_core::{Error, Result};
use sqlx::{Acquire, Sqlite};

use super::*;

#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "INSERT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn bulk_create<'a, A, M>(executor: A, models: &[M]) -> Result<usize>
where
    A: Acquire<'a, Database = Sqlite>,
    M: Model + ModelValues,
{
    if models.is_empty() {
        return Ok(0);
    }
    let n_cols = rango_orm_core::insert_values(&models[0]).len();
    let chunk_size = (999 / n_cols).max(1); // SQLite limit: 999 bound params
    let mut total = 0usize;
    // One transaction for all chunks (a savepoint when `executor` is already a transaction):
    // a failure in a later chunk must not leave the earlier rows behind.
    let mut tx = executor.begin().await.map_err(db_err)?;
    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_create_sql::<M>(chunk);
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), values);
        let result = q.execute(&mut *tx).await.map_err(db_err)?;
        total += result.rows_affected() as usize;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(total)
}

/// Columns come from the first model: give every model a PK, or none (auto-increment).
fn build_bulk_create_sql<M: Model + ModelValues>(models: &[M]) -> (String, Vec<SqlValue>) {
    let cols: Vec<String> = rango_orm_core::insert_values(&models[0])
        .into_iter()
        .map(|(c, _)| quote_ident(c))
        .collect();
    let mut all_values: Vec<SqlValue> = Vec::new();
    let mut row_placeholders: Vec<String> = Vec::new();
    for model in models {
        let values = rango_orm_core::insert_values(model);
        let ph: Vec<&str> = vec!["?"; values.len()];
        all_values.extend(values.into_iter().map(|(_, v)| v));
        row_placeholders.push(format!("({})", ph.join(", ")));
    }
    let sql = format!(
        "INSERT INTO {} ({}) VALUES {}",
        quote_ident(M::table_name()),
        cols.join(", "),
        row_placeholders.join(", ")
    );
    (sql, all_values)
}

/// UPDATE multiple rows by primary key, in one transaction (a savepoint inside `atomic`).
///
/// `fields` lists the columns to update (never the PK, it is the join key). SQLite has no `UPDATE ... FROM (VALUES ...)` with column aliases usable everywhere, so this runs one prepared `UPDATE` per row (in-process, cheap).
/// Returns the number of rows matched; unknown columns fail with `Error::Invalid` before
/// anything runs.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "UPDATE", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn bulk_update<'a, A, M>(executor: A, models: &[M], fields: &[&str]) -> Result<usize>
where
    A: Acquire<'a, Database = Sqlite>,
    M: Model + ModelValues,
{
    if models.is_empty() || fields.is_empty() {
        return Ok(0);
    }
    for f in fields {
        if let Some(msg) = rango_orm_core::unknown_column::<M>(f) {
            return Err(Error::Invalid(msg));
        }
        if *f == M::pk_column() {
            return Err(Error::Invalid(format!(
                "`{f}` is the primary key and cannot be updated"
            )));
        }
    }
    let sql = format!(
        "UPDATE {} SET {} WHERE {} = ?",
        quote_ident(M::table_name()),
        fields
            .iter()
            .map(|f| format!("{} = ?", quote_ident(f)))
            .collect::<Vec<_>>()
            .join(", "),
        quote_ident(M::pk_column())
    );
    let mut total = 0usize;
    let mut tx = executor.begin().await.map_err(db_err)?;
    for model in models {
        let all = model.field_values();
        let mut values: Vec<SqlValue> = fields
            .iter()
            .map(|f| {
                all.iter()
                    .find(|(c, _)| c == f)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(SqlValue::NullText)
            })
            .collect();
        values.push(model.pk_value());
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), values);
        total += q.execute(&mut *tx).await.map_err(db_err)?.rows_affected() as usize;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(total)
}

/// INSERT multiple rows with `ON CONFLICT ... DO UPDATE`.
///
/// `conflict_on` names the columns that detect a conflict (UNIQUE index or PK); every other column is overwritten on conflict. Auto-chunked under the placeholder limit, in one transaction. Returns the number
/// of rows affected.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "INSERT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn bulk_upsert<'a, A, M>(executor: A, models: &[M], conflict_on: &[&str]) -> Result<usize>
where
    A: Acquire<'a, Database = Sqlite>,
    M: Model + ModelValues,
{
    if models.is_empty() {
        return Ok(0);
    }
    for c in conflict_on {
        if let Some(msg) = rango_orm_core::unknown_column::<M>(c) {
            return Err(Error::Invalid(msg));
        }
    }
    let n_cols = rango_orm_core::insert_values(&models[0]).len();
    let chunk_size = (999 / n_cols).max(1);
    let mut total = 0usize;
    let mut tx = executor.begin().await.map_err(db_err)?;
    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_upsert_sql::<M>(chunk, conflict_on);
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), values);
        total += q.execute(&mut *tx).await.map_err(db_err)?.rows_affected() as usize;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(total)
}

fn build_bulk_upsert_sql<M: Model + ModelValues>(
    models: &[M],
    conflict_on: &[&str],
) -> (String, Vec<SqlValue>) {
    let (insert, values) = build_bulk_create_sql::<M>(models);
    let mut update_cols: Vec<String> = rango_orm_core::insert_values(&models[0])
        .into_iter()
        .map(|(c, _)| c)
        .filter(|c| *c != M::pk_column() && !conflict_on.contains(c))
        .map(|c| format!("{0} = excluded.{0}", quote_ident(c)))
        .collect();
    let conflict = conflict_on
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    // Without conflict columns SQLite needs the PK as the conflict target.
    let conflict = if conflict.is_empty() {
        quote_ident(M::pk_column())
    } else {
        conflict
    };
    let action = if update_cols.is_empty() {
        "DO NOTHING".to_string()
    } else {
        format!(
            "DO UPDATE SET {}",
            std::mem::take(&mut update_cols).join(", ")
        )
    };
    (
        format!("{insert} ON CONFLICT ({conflict}) {action}"),
        values,
    )
}
