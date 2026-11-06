//! Multi-row operations: bulk insert, update and upsert.

use crate::error::db_err;
use rango_orm_core::Result;
use rango_orm_core::{Model, ModelValues, SqlValue, quote_ident};
use sqlx::{Acquire, Postgres};

use super::*;

// ─── Bulk ops ─────────────────────────────────────────────────────────────────
/// INSERT multiple rows in a single query.
///
/// Automatically chunks to stay within PostgreSQL's 65 535 bind-parameter limit.
/// Returns the total number of rows inserted.
///
/// # Example
/// ```rust,ignore
/// let n = rango::bulk_create(&pool, &users).await?;
/// ```
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "INSERT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn bulk_create<'a, A, M>(executor: A, models: &[M]) -> Result<usize>
where
    A: Acquire<'a, Database = Postgres>,
    M: Model + ModelValues,
{
    if models.is_empty() {
        return Ok(0);
    }

    let mut conn = executor.acquire().await.map_err(db_err)?;

    let n_cols = rango_orm_core::insert_values(&models[0]).len();
    let chunk_size = (65535 / n_cols).max(1);
    let mut total = 0usize;

    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_create_sql::<M>(chunk);
        let q = bind_sql_values(sqlx::query(&sql), values);
        let result = q.execute(&mut *conn).await.map_err(db_err)?;
        total += result.rows_affected() as usize;
    }
    Ok(total)
}

/// UPDATE multiple rows using a single `UPDATE … FROM (VALUES …)` query.
///
/// `fields` lists the columns to update (not the PK — it is always used as the
/// join key). Automatically chunks to stay within PostgreSQL's bind limit.
/// Returns the total number of rows updated.
///
/// # Example
/// ```rust,ignore
/// let n = rango::bulk_update(&pool, &users, &["email", "status"]).await?;
/// ```
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "UPDATE", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn bulk_update<'a, A, M>(executor: A, models: &[M], fields: &[&str]) -> Result<usize>
where
    A: Acquire<'a, Database = Postgres>,
    M: Model + ModelValues,
{
    if models.is_empty() || fields.is_empty() {
        return Ok(0);
    }

    let mut conn = executor.acquire().await.map_err(db_err)?;

    let n_cols = fields.len() + 1; // +1 for pk
    let chunk_size = (65535 / n_cols).max(1);
    let mut total = 0usize;

    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_update_sql::<M>(chunk, fields);
        let q = bind_sql_values(sqlx::query(&sql), values);
        let result = q.execute(&mut *conn).await.map_err(db_err)?;
        total += result.rows_affected() as usize;
    }
    Ok(total)
}

/// INSERT multiple rows with `ON CONFLICT … DO UPDATE`.
///
/// `conflict_on` names the columns used to detect conflicts (must be covered by
/// a UNIQUE constraint). All other non-PK, non-conflict columns are updated on
/// conflict. Automatically chunks to stay within PostgreSQL's bind limit.
/// Returns the total number of rows upserted.
///
/// # Example
/// ```rust,ignore
/// let n = rango::bulk_upsert(&pool, &users, &["email"]).await?;
/// ```
#[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "INSERT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
pub async fn bulk_upsert<'a, A, M>(executor: A, models: &[M], conflict_on: &[&str]) -> Result<usize>
where
    A: Acquire<'a, Database = Postgres>,
    M: Model + ModelValues,
{
    if models.is_empty() {
        return Ok(0);
    }

    let mut conn = executor.acquire().await.map_err(db_err)?;

    let n_cols = 1 + models[0].field_values().len();
    let chunk_size = (65535 / n_cols).max(1);
    let mut total = 0usize;

    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_upsert_sql::<M>(chunk, conflict_on);
        let q = bind_sql_values(sqlx::query(&sql), values);
        let result = q.execute(&mut *conn).await.map_err(db_err)?;
        total += result.rows_affected() as usize;
    }
    Ok(total)
}

// ─── Bulk SQL builders ────────────────────────────────────────────────────────
fn sql_type_cast(val: &SqlValue) -> &'static str {
    match val {
        SqlValue::NullBool
        | SqlValue::NullSmallInt
        | SqlValue::NullInt
        | SqlValue::NullBigInt
        | SqlValue::NullFloat
        | SqlValue::NullDouble
        | SqlValue::NullText
        | SqlValue::NullBytes
        | SqlValue::NullUuid
        | SqlValue::NullDateTime
        | SqlValue::NullDate
        | SqlValue::NullTime
        | SqlValue::NullJson => "::text",
        SqlValue::Bool(_) => "::bool",
        SqlValue::SmallInt(_) => "::int2",
        SqlValue::Int(_) => "::int4",
        SqlValue::BigInt(_) => "::int8",
        SqlValue::Float(_) => "::float4",
        SqlValue::Double(_) => "::float8",
        SqlValue::Text(_) => "::text",
        SqlValue::Bytes(_) => "::bytea",
        SqlValue::Uuid(_) => "::uuid",
        SqlValue::DateTime(_) => "::timestamptz",
        SqlValue::Date(_) => "::date",
        SqlValue::Time(_) => "::time",
        SqlValue::Json(_) => "::jsonb",
    }
}

/// Columns come from the first model: give every model a PK, or none (auto-increment).
fn build_bulk_create_sql<M: Model + ModelValues>(models: &[M]) -> (String, Vec<SqlValue>) {
    let cols: Vec<String> = rango_orm_core::insert_values(&models[0])
        .into_iter()
        .map(|(c, _)| quote_ident(c))
        .collect();

    let mut all_values: Vec<SqlValue> = Vec::new();
    let mut row_placeholders: Vec<String> = Vec::new();
    let mut idx = 1usize;

    for model in models {
        let values = rango_orm_core::insert_values(model);

        let mut ph: Vec<String> = Vec::new();
        for _ in &values {
            ph.push(format!("${}", idx));
            idx += 1;
        }
        all_values.extend(values.into_iter().map(|(_, v)| v));
        row_placeholders.push(format!("({})", ph.join(", ")));
    }

    let sql = format!(
        "INSERT INTO {} ({}) VALUES {}",
        quote_ident(M::table_name()),
        cols.join(", "),
        row_placeholders.join(", "),
    );
    (sql, all_values)
}

fn build_bulk_update_sql<M: Model + ModelValues>(
    models: &[M],
    fields: &[&str],
) -> (String, Vec<SqlValue>) {
    let table = M::table_name();
    let pk_col = M::pk_column();

    let mut all_values: Vec<SqlValue> = Vec::new();
    let mut row_placeholders: Vec<String> = Vec::new();
    let mut idx = 1usize;

    for (row_i, model) in models.iter().enumerate() {
        let model_fields = model.field_values();
        let pk_val = model.pk_value();
        let first = row_i == 0;

        let mut ph: Vec<String> = Vec::new();

        for f in fields.iter() {
            let val = model_fields
                .iter()
                .find(|(c, _)| c == f)
                .map(|(_, v)| v.clone())
                .unwrap_or(SqlValue::NullText);
            let cast = if first { sql_type_cast(&val) } else { "" };
            ph.push(format!("${}{}", idx, cast));
            all_values.push(val);
            idx += 1;
        }

        let pk_cast = if first { sql_type_cast(&pk_val) } else { "" };
        ph.push(format!("${}{}", idx, pk_cast));
        all_values.push(pk_val);
        idx += 1;

        row_placeholders.push(format!("({})", ph.join(", ")));
    }

    let set_clauses: Vec<String> = fields
        .iter()
        .map(|f| format!("{} = v.{}", quote_ident(f), quote_ident(f)))
        .collect();

    let alias_cols: Vec<String> = fields
        .iter()
        .map(|f| quote_ident(f))
        .chain(std::iter::once(quote_ident(pk_col)))
        .collect();

    let sql = format!(
        "UPDATE {table} SET {set} FROM (VALUES {vals}) AS v({alias}) WHERE {table}.{pk} = v.{pk}",
        table = quote_ident(table),
        set = set_clauses.join(", "),
        vals = row_placeholders.join(", "),
        alias = alias_cols.join(", "),
        pk = quote_ident(pk_col),
    );
    (sql, all_values)
}

fn build_bulk_upsert_sql<M: Model + ModelValues>(
    models: &[M],
    conflict_on: &[&str],
) -> (String, Vec<SqlValue>) {
    let pk_col = M::pk_column();
    let sample = models[0].field_values();

    let mut cols = vec![quote_ident(pk_col)];
    cols.extend(sample.iter().map(|(c, _)| quote_ident(c)));

    let update_cols: Vec<String> = sample
        .iter()
        .filter(|(c, _)| !conflict_on.contains(c))
        .map(|(c, _)| format!("{} = EXCLUDED.{}", quote_ident(c), quote_ident(c)))
        .collect();

    let mut all_values: Vec<SqlValue> = Vec::new();
    let mut row_placeholders: Vec<String> = Vec::new();
    let mut idx = 1usize;

    for model in models {
        let fields = model.field_values();
        let pk_val = model.pk_value();

        let mut ph: Vec<String> = vec![format!("${}", idx)];
        all_values.push(pk_val);
        idx += 1;

        for (_, val) in fields {
            ph.push(format!("${}", idx));
            all_values.push(val);
            idx += 1;
        }
        row_placeholders.push(format!("({})", ph.join(", ")));
    }

    let conflict_clause = conflict_on
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");

    let sql = if update_cols.is_empty() {
        format!(
            "INSERT INTO {} ({}) VALUES {} ON CONFLICT ({}) DO NOTHING",
            quote_ident(M::table_name()),
            cols.join(", "),
            row_placeholders.join(", "),
            conflict_clause,
        )
    } else {
        format!(
            "INSERT INTO {} ({}) VALUES {} ON CONFLICT ({}) DO UPDATE SET {}",
            quote_ident(M::table_name()),
            cols.join(", "),
            row_placeholders.join(", "),
            conflict_clause,
            update_cols.join(", "),
        )
    };
    (sql, all_values)
}
