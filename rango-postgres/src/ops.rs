use rango_core::{FromRow, Model, ModelValues, SqlValue};
use sqlx::{Executor, Postgres};
use sqlx::postgres::PgArguments;
use sqlx::query::Query;
use anyhow::{Context, Result};

use crate::executor::RangoExecutor;
use crate::row::PgRangoRow;

// ─── Bind macro ───────────────────────────────────────────────────────────────

macro_rules! bind {
    ($q:expr, $val:expr) => {
        match $val {
            SqlValue::Null         => $q.bind(Option::<String>::None),
            SqlValue::Bool(v)      => $q.bind(v),
            SqlValue::SmallInt(v)  => $q.bind(v),
            SqlValue::Int(v)       => $q.bind(v),
            SqlValue::BigInt(v)    => $q.bind(v),
            SqlValue::Float(v)     => $q.bind(v),
            SqlValue::Double(v)    => $q.bind(v),
            SqlValue::Text(v)      => $q.bind(v),
            SqlValue::Bytes(v)     => $q.bind(v),
            SqlValue::Uuid(v)      => $q.bind(v),
            SqlValue::DateTime(v)  => $q.bind(v),
            SqlValue::Date(v)      => $q.bind(v),
            SqlValue::Time(v)      => $q.bind(v),
            SqlValue::Json(v)      => $q.bind(sqlx::types::Json(v)),
        }
    };
}

pub fn bind_sql_values<'q>(
    mut q: Query<'q, Postgres, PgArguments>,
    values: Vec<SqlValue>,
) -> Query<'q, Postgres, PgArguments> {
    for val in values { q = bind!(q, val); }
    q
}

// ─── Single-query ops — transparent over PgPool, Transaction, and any connection ─

/// INSERT → returns the model as stored.
/// Accepts `&PgPool`, `&mut Transaction<'_, Postgres>`, or any sqlx `Executor`.
///
/// # Example
/// ```rust
/// // With pool
/// let user = rango::insert(&pool, user).await?;
///
/// // Inside rango::atomic()
/// let user = rango::atomic(&pool, |tx| async move {
///     let user = rango::insert(tx, user).await?;
///     rango::insert(tx, profile).await?;
///     Ok(user)
/// }).await?;
/// ```
pub async fn insert<'e, E, M>(executor: E, model: M) -> Result<M>
where
    E: Executor<'e, Database = Postgres>,
    M: Model + ModelValues + FromRow,
{
    let (sql, values) = build_insert_sql::<M>(&model);
    let q = bind_sql_values(sqlx::query(&sql), values);
    let row = q.fetch_one(executor).await
        .with_context(|| format!("INSERT into {} failed", M::table_name()))?;
    M::from_row(&PgRangoRow(row)).map_err(|e| anyhow::anyhow!("{}", e))
}

/// UPDATE → returns the updated model.
pub async fn update<'e, E, M>(executor: E, model: M) -> Result<M>
where
    E: Executor<'e, Database = Postgres>,
    M: Model + ModelValues + FromRow,
{
    let (sql, values) = build_update_sql::<M>(&model);
    let q = bind_sql_values(sqlx::query(&sql), values);
    let row = q.fetch_one(executor).await
        .with_context(|| format!("UPDATE {} failed", M::table_name()))?;
    M::from_row(&PgRangoRow(row)).map_err(|e| anyhow::anyhow!("{}", e))
}

/// DELETE a model.
pub async fn delete<'e, E, M>(executor: E, model: &M) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
    M: Model + ModelValues,
{
    let (sql, values) = build_delete_sql::<M>(model);
    let q = bind_sql_values(sqlx::query(&sql), values);
    q.execute(executor).await
        .map(|_| ())
        .with_context(|| format!("DELETE from {} failed", M::table_name()))
}

/// GET by primary key.
/// Accepts `&PgPool`, `&mut Transaction<'_, Postgres>`, or any sqlx `Executor`.
pub async fn get<'e, E, M>(executor: E, pk: &SqlValue) -> Result<Option<M>>
where
    E: Executor<'e, Database = Postgres>,
    M: Model + ModelValues + FromRow,
{
    let sql = format!(
        "SELECT * FROM \"{}\" WHERE \"{}\" = $1 LIMIT 1",
        M::table_name(), M::pk_column()
    );
    let q = bind_sql_values(sqlx::query(&sql), vec![pk.clone()]);
    let row = q.fetch_optional(executor).await
        .with_context(|| format!("GET from {} failed", M::table_name()))?;
    match row {
        Some(r) => Ok(Some(M::from_row(&PgRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e))?)),
        None => Ok(None),
    }
}

/// GET ALL rows.
/// Accepts `&PgPool`, `&mut Transaction<'_, Postgres>`, or any sqlx `Executor`.
pub async fn all<'e, E, M>(executor: E) -> Result<Vec<M>>
where
    E: Executor<'e, Database = Postgres>,
    M: Model + ModelValues + FromRow,
{
    let sql = format!("SELECT * FROM \"{}\"", M::table_name());
    let rows = sqlx::query(&sql).fetch_all(executor).await
        .with_context(|| format!("ALL from {} failed", M::table_name()))?;
    rows.into_iter()
        .map(|r| M::from_row(&PgRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e)))
        .collect()
}

// ─── Compound ops — require RangoExecutor (pool or transaction, sequential queries) ─

/// GET OR CREATE — returns (model, created: bool).
///
/// Accepts `&mut pool` (for standalone use) or `tx` (inside `atomic()`).
///
/// Note: to guarantee atomicity (no race between SELECT and INSERT), wrap in `atomic()`.
///
/// # Example
/// ```rust
/// // Standalone (not atomic — races possible under high concurrency)
/// let (user, created) = rango::get_or_create(&mut pool, lookup, defaults).await?;
///
/// // Atomic
/// let (user, created) = rango::atomic(&pool, |tx| async move {
///     rango::get_or_create(tx, lookup, defaults).await
/// }).await?;
/// ```
pub async fn get_or_create<E, M>(
    exec: &mut E,
    lookup: Vec<(&'static str, SqlValue)>,
    defaults: M,
) -> Result<(M, bool)>
where
    E: RangoExecutor,
    M: Model + ModelValues + FromRow,
{
    let (lookup_sql, lookup_vals) = build_lookup_sql::<M>(&lookup);
    let q = bind_sql_values(sqlx::query(&lookup_sql), lookup_vals);
    let row = exec.fetch_optional_query(q).await
        .with_context(|| format!("GET_OR_CREATE lookup on {} failed", M::table_name()))?;
    if let Some(r) = row {
        return Ok((M::from_row(&PgRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e))?, false));
    }
    let (insert_sql, insert_vals) = build_insert_sql::<M>(&defaults);
    let q = bind_sql_values(sqlx::query(&insert_sql), insert_vals);
    let row = exec.fetch_one_query(q).await
        .with_context(|| format!("GET_OR_CREATE insert on {} failed", M::table_name()))?;
    Ok((M::from_row(&PgRangoRow(row)).map_err(|e| anyhow::anyhow!("{}", e))?, true))
}

/// UPDATE OR CREATE.
///
/// Accepts `&mut pool` (for standalone use) or `tx` (inside `atomic()`).
/// For atomicity, wrap in `atomic()`.
pub async fn update_or_create<E, M>(
    exec: &mut E,
    lookup: Vec<(&'static str, SqlValue)>,
    values: M,
) -> Result<M>
where
    E: RangoExecutor,
    M: Model + ModelValues + FromRow,
{
    let (lookup_sql, lookup_vals) = build_lookup_sql::<M>(&lookup);
    let q = bind_sql_values(sqlx::query(&lookup_sql), lookup_vals);
    let row = exec.fetch_optional_query(q).await
        .with_context(|| format!("UPDATE_OR_CREATE lookup on {} failed", M::table_name()))?;
    if row.is_some() {
        let (update_sql, update_vals) = build_update_sql::<M>(&values);
        let q = bind_sql_values(sqlx::query(&update_sql), update_vals);
        let row = exec.fetch_one_query(q).await
            .with_context(|| format!("UPDATE_OR_CREATE update on {} failed", M::table_name()))?;
        M::from_row(&PgRangoRow(row)).map_err(|e| anyhow::anyhow!("{}", e))
    } else {
        let (insert_sql, insert_vals) = build_insert_sql::<M>(&values);
        let q = bind_sql_values(sqlx::query(&insert_sql), insert_vals);
        let row = exec.fetch_one_query(q).await
            .with_context(|| format!("UPDATE_OR_CREATE insert on {} failed", M::table_name()))?;
        M::from_row(&PgRangoRow(row)).map_err(|e| anyhow::anyhow!("{}", e))
    }
}

// ─── SQL builders ─────────────────────────────────────────────────────────────

fn build_insert_sql<M: Model + ModelValues>(model: &M) -> (String, Vec<SqlValue>) {
    let fields = model.field_values();
    let pk_col = M::pk_column();
    let pk_val = model.pk_value();
    let mut cols = vec![format!("\"{}\"", pk_col)];
    cols.extend(fields.iter().map(|(c, _)| format!("\"{}\"", c)));
    let placeholders: Vec<String> = (1..=cols.len()).map(|i| format!("${}", i)).collect();
    let sql = format!(
        "INSERT INTO \"{}\" ({}) VALUES ({}) RETURNING *",
        M::table_name(), cols.join(", "), placeholders.join(", ")
    );
    let mut values = vec![pk_val];
    values.extend(fields.into_iter().map(|(_, v)| v));
    (sql, values)
}

fn build_update_sql<M: Model + ModelValues>(model: &M) -> (String, Vec<SqlValue>) {
    let fields = model.field_values();
    let pk_col = M::pk_column();
    let pk_val = model.pk_value();
    let set_clauses: Vec<String> = fields.iter().enumerate()
        .map(|(i, (col, _))| format!("\"{}\" = ${}", col, i + 1))
        .collect();
    let sql = format!(
        "UPDATE \"{}\" SET {} WHERE \"{}\" = ${} RETURNING *",
        M::table_name(), set_clauses.join(", "), pk_col, fields.len() + 1
    );
    let mut values: Vec<SqlValue> = fields.into_iter().map(|(_, v)| v).collect();
    values.push(pk_val);
    (sql, values)
}

fn build_delete_sql<M: Model + ModelValues>(model: &M) -> (String, Vec<SqlValue>) {
    (
        format!("DELETE FROM \"{}\" WHERE \"{}\" = $1", M::table_name(), M::pk_column()),
        vec![model.pk_value()],
    )
}

fn build_lookup_sql<M: Model + ModelValues>(
    lookup: &[(&'static str, SqlValue)],
) -> (String, Vec<SqlValue>) {
    let conditions: Vec<String> = lookup.iter().enumerate()
        .map(|(i, (col, _))| format!("\"{}\" = ${}", col, i + 1))
        .collect();
    let sql = format!(
        "SELECT * FROM \"{}\" WHERE {} LIMIT 1",
        M::table_name(), conditions.join(" AND ")
    );
    let values = lookup.iter().map(|(_, v)| v.clone()).collect();
    (sql, values)
}
