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
    run_chunks(executor, models, chunk_size, |chunk| {
        build_bulk_create_sql::<M>(chunk)
    })
    .await
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

/// UPDATE multiple rows by primary key: one `UPDATE ... FROM (VALUES ...)` per chunk; several chunks run in
/// one transaction (a savepoint inside `atomic`), a single chunk in none (one statement is atomic).
///
/// `fields` lists the columns to update (never the PK, it is the join key). Auto-chunked under the placeholder limit.
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
    let chunk_size = (999 / (fields.len() + 1)).max(1);
    run_chunks(executor, models, chunk_size, |chunk| {
        build_bulk_update_sql::<M>(chunk, fields)
    })
    .await
}

/// `WITH v(pk, c1, ..) AS (VALUES (?, ?, ..), ..) UPDATE t SET c1 = v.c1, .. FROM v WHERE t.pk = v.pk`
fn build_bulk_update_sql<M: Model + ModelValues>(
    models: &[M],
    fields: &[&str],
) -> (String, Vec<SqlValue>) {
    let mut values: Vec<SqlValue> = Vec::with_capacity(models.len() * (fields.len() + 1));
    for model in models {
        let all = model.field_values();
        values.push(model.pk_value());
        values.extend(fields.iter().map(|f| {
            all.iter()
                .find(|(c, _)| c == f)
                .map(|(_, v)| v.clone())
                .unwrap_or(SqlValue::NullText)
        }));
    }
    let row = format!("({})", vec!["?"; fields.len() + 1].join(", "));
    let pk = quote_ident(M::pk_column());
    let table = quote_ident(M::table_name());
    let cols = fields.iter().map(|f| quote_ident(f)).collect::<Vec<_>>();
    let sql = format!(
        "WITH v({pk}, {cols}) AS (VALUES {rows}) UPDATE {table} SET {set} FROM v WHERE {table}.{pk} = v.{pk}",
        cols = cols.join(", "),
        rows = vec![row; models.len()].join(", "),
        set = cols
            .iter()
            .map(|c| format!("{c} = v.{c}"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    (sql, values)
}

/// INSERT multiple rows with `ON CONFLICT ... DO UPDATE`.
///
/// `conflict_on` names the columns that detect a conflict (UNIQUE index or PK); every other column is overwritten on conflict. Auto-chunked under the placeholder limit; several chunks share one transaction, a single chunk needs none. Returns the number
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
    run_chunks(executor, models, chunk_size, |chunk| {
        build_bulk_upsert_sql::<M>(chunk, conflict_on)
    })
    .await
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

/// Runs one statement per chunk. A single chunk is one statement, already atomic: no
/// transaction. Several chunks share one transaction (a savepoint when `executor` is already
/// a transaction), so a later failure does not leave the earlier rows behind.
async fn run_chunks<'a, A, M>(
    executor: A,
    models: &[M],
    chunk_size: usize,
    build: impl Fn(&[M]) -> (String, Vec<SqlValue>),
) -> Result<usize>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let run = |sql: String, values| bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(sql)), values);
    if models.len() <= chunk_size {
        let mut conn = executor.acquire().await.map_err(db_err)?;
        let (sql, values) = build(models);
        let result = run(sql, values).execute(&mut *conn).await.map_err(db_err)?;
        return Ok(result.rows_affected() as usize);
    }
    let mut total = 0usize;
    let mut tx = executor.begin().await.map_err(db_err)?;
    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build(chunk);
        let result = run(sql, values).execute(&mut *tx).await.map_err(db_err)?;
        total += result.rows_affected() as usize;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(total)
}
