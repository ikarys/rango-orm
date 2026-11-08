//! Multi-row operations: bulk insert, update and upsert.

use crate::error::db_err;
use rango_orm_core::{quote_ident_mysql as quote_ident, Model, ModelValues, SqlValue};
use rango_orm_core::{Error, Result};
use sqlx::{Acquire, MySql};

use super::*;

#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "INSERT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn bulk_create<'a, A, M>(executor: A, models: &[M]) -> Result<usize>
where
    A: Acquire<'a, Database = MySql>,
    M: Model + ModelValues,
{
    if models.is_empty() {
        return Ok(0);
    }
    let n_cols = rango_orm_core::insert_values(&models[0]).len();
    let chunk_size = (65_535 / n_cols).max(1); // MySQL limit: 65 535 placeholders per statement
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

/// Rows per `bulk_update` statement (see the comment in `bulk_update`).
const MAX_UPDATE_CHUNK: usize = 100;

/// UPDATE multiple rows by primary key: one `UPDATE ... SET c = CASE pk WHEN ? THEN ? ... END
/// WHERE pk IN (...)` per chunk; several chunks run in one transaction (a savepoint inside `atomic`), a single chunk in none (one statement is atomic).
///
/// `fields` lists the columns to update (never the PK, it is the join key). `CASE` is used instead of a derived table because MySQL and MariaDB infer the types and collations of a `VALUES` table differently; here every bound value is converted by the target column. Auto-chunked (100 rows, or the placeholder limit). sqlx sets `CLIENT_FOUND_ROWS`, so unchanged rows still count.
/// Returns the number of rows matched; unknown columns fail with `Error::Invalid` before
/// anything runs.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "UPDATE", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn bulk_update<'a, A, M>(executor: A, models: &[M], fields: &[&str]) -> Result<usize>
where
    A: Acquire<'a, Database = MySql>,
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
    // Per row: 1 placeholder in `IN (...)` plus a WHEN/THEN pair per column.
    // A `CASE` is scanned linearly for every row, so the cost per chunk is quadratic: capping the
    // chunk at 100 rows was ~7x faster than filling the placeholder limit (measured, 5000 rows).
    let chunk_size = (65_535 / (2 * fields.len() + 1)).clamp(1, MAX_UPDATE_CHUNK);
    run_chunks(executor, models, chunk_size, |chunk| {
        build_bulk_update_sql::<M>(chunk, fields)
    })
    .await
}

/// `UPDATE t SET c1 = CASE pk WHEN ? THEN ? .. END, .. WHERE pk IN (?, ..)`
fn build_bulk_update_sql<M: Model + ModelValues>(
    models: &[M],
    fields: &[&str],
) -> (String, Vec<SqlValue>) {
    let pk = quote_ident(M::pk_column());
    let all: Vec<_> = models.iter().map(|m| m.field_values()).collect();
    let mut values: Vec<SqlValue> = Vec::with_capacity(models.len() * (2 * fields.len() + 1));
    let mut set: Vec<String> = Vec::with_capacity(fields.len());
    for f in fields {
        let when = "WHEN ? THEN ? ".repeat(models.len());
        set.push(format!("{c} = CASE {pk} {when}END", c = quote_ident(f)));
        for (model, vals) in models.iter().zip(&all) {
            values.push(model.pk_value());
            values.push(
                vals.iter()
                    .find(|(c, _)| c == f)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(SqlValue::NullText),
            );
        }
    }
    values.extend(models.iter().map(|m| m.pk_value()));
    let sql = format!(
        "UPDATE {} SET {} WHERE {pk} IN ({})",
        quote_ident(M::table_name()),
        set.join(", "),
        vec!["?"; models.len()].join(", "),
    );
    (sql, values)
}

/// INSERT multiple rows with `ON DUPLICATE KEY UPDATE`.
///
/// MySQL fires on a duplicate of any unique index or the PK, whatever `conflict_on` says: the argument only lists columns that are not overwritten. Auto-chunked under the placeholder limit; several chunks share one transaction, a single chunk needs none. Returns the number
/// of rows affected.
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "mysql", db.operation = "INSERT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn bulk_upsert<'a, A, M>(executor: A, models: &[M], conflict_on: &[&str]) -> Result<usize>
where
    A: Acquire<'a, Database = MySql>,
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
    check_upsert_keys::<M>(conflict_on)?;
    let n_cols = rango_orm_core::insert_values(&models[0]).len();
    let chunk_size = (65_535 / n_cols).max(1);
    run_chunks(executor, models, chunk_size, |chunk| {
        build_bulk_upsert_sql::<M>(chunk, conflict_on)
    })
    .await
}

/// `ON DUPLICATE KEY UPDATE` fires on any unique key and cannot name its target, so
/// reject tables with a unique key other than `conflict_on` (the PK is exempt: a PK
/// collision is the same row). Partial unique constraints are not created on MySQL.
fn check_upsert_keys<M: Model>(conflict_on: &[&str]) -> Result<()> {
    use rango_orm_core::Constraint;
    let schema = M::schema();
    let mut wanted: Vec<&str> = conflict_on.to_vec();
    wanted.sort_unstable();
    wanted.dedup();
    let single = schema
        .columns
        .iter()
        .filter(|c| c.unique && !c.primary_key)
        .map(|c| vec![c.name.as_str()]);
    let composite = schema.constraints.iter().filter_map(|c| match c {
        Constraint::Unique(u) if u.condition.is_none() => {
            Some(u.fields.iter().map(String::as_str).collect())
        }
        _ => None,
    });
    let others: Vec<String> = single
        .chain(composite)
        .filter(|k: &Vec<&str>| {
            let mut k = k.clone();
            k.sort_unstable();
            k.dedup();
            k != wanted
        })
        .map(|k| format!("({})", k.join(", ")))
        .collect();
    if others.is_empty() {
        return Ok(());
    }
    Err(Error::Invalid(format!(
        "bulk_upsert on `{}`: MySQL `ON DUPLICATE KEY UPDATE` fires on any unique key and cannot \
         target ({}), but the table also has unique key(s) {}; use another approach (e.g. \
         update_or_create) or a model with a single unique key",
        M::table_name(),
        conflict_on.join(", "),
        others.join(", ")
    )))
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
        .map(|c| format!("{0} = VALUES({0})", quote_ident(c)))
        .collect();
    if update_cols.is_empty() {
        // No-op assignment: a duplicate leaves the row alone without `INSERT IGNORE`.
        let pk = quote_ident(M::pk_column());
        update_cols.push(format!("{pk} = {pk}"));
    }
    (
        format!(
            "{insert} ON DUPLICATE KEY UPDATE {}",
            update_cols.join(", ")
        ),
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
    A: Acquire<'a, Database = MySql>,
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
