use rango_core::{FromRow, Model, ModelValues, SqlValue};
use sqlx::{Acquire, Executor, Postgres};

/// Default maximum rows returned by [`QueryBuilder::all()`].
/// Override with `.limit(n)` or `.unlimited()`.
pub const DEFAULT_QUERY_LIMIT: i64 = 1000;
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
/// ```rust,ignore
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
/// let (user, created) = rango::atomic(&pool, |tx| async move {
///     rango::get_or_create(&mut *tx, lookup, defaults).await
/// }).await?;
/// ```
pub async fn get_or_create<'a, A, M>(
    executor: A,
    lookup: Vec<(&'static str, SqlValue)>,
    defaults: M,
) -> Result<(M, bool)>
where
    A: Acquire<'a, Database = Postgres>,
    M: Model + ModelValues + FromRow,
{
    let mut conn = executor.acquire().await
        .with_context(|| format!("get_or_create: failed to acquire connection for {}", M::table_name()))?;

    let conflict_cols: Vec<&str> = lookup.iter().map(|(col, _)| *col).collect();
    let (insert_sql, insert_vals) = build_get_or_create_sql(&defaults, &conflict_cols);
    let q = bind_sql_values(sqlx::query(&insert_sql), insert_vals);
    let row = q.fetch_optional(&mut *conn).await
        .with_context(|| format!("get_or_create INSERT on {} failed", M::table_name()))?;

    if let Some(r) = row {
        return Ok((M::from_row(&PgRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e))?, true));
    }

    // Conflict — row already exists; fetch it
    let (select_sql, select_vals) = build_lookup_sql::<M>(&lookup);
    let q = bind_sql_values(sqlx::query(&select_sql), select_vals);
    let row = q.fetch_one(&mut *conn).await
        .with_context(|| format!("get_or_create SELECT fallback on {} failed", M::table_name()))?;

    Ok((M::from_row(&PgRangoRow(row)).map_err(|e| anyhow::anyhow!("{}", e))?, false))
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

fn build_get_or_create_sql<M: Model + ModelValues>(
    model: &M,
    conflict_cols: &[&str],
) -> (String, Vec<SqlValue>) {
    let fields = model.field_values();
    let pk_col = M::pk_column();
    let pk_val = model.pk_value();
    let mut cols = vec![format!("\"{}\"", pk_col)];
    cols.extend(fields.iter().map(|(c, _)| format!("\"{}\"", c)));
    let placeholders: Vec<String> = (1..=cols.len()).map(|i| format!("${}", i)).collect();
    let conflict_clause = conflict_cols.iter()
        .map(|c| format!("\"{}\"", c))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT INTO \"{}\" ({}) VALUES ({}) ON CONFLICT ({}) DO NOTHING RETURNING *",
        M::table_name(), cols.join(", "), placeholders.join(", "), conflict_clause
    );
    let mut values = vec![pk_val];
    values.extend(fields.into_iter().map(|(_, v)| v));
    (sql, values)
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
pub async fn bulk_create<'a, A, M>(executor: A, models: &[M]) -> Result<usize>
where
    A: Acquire<'a, Database = Postgres>,
    M: Model + ModelValues,
{
    if models.is_empty() { return Ok(0); }

    let mut conn = executor.acquire().await
        .with_context(|| format!("bulk_create: failed to acquire connection for {}", M::table_name()))?;

    let n_cols = 1 + models[0].field_values().len();
    let chunk_size = (65535 / n_cols).max(1);
    let mut total = 0usize;

    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_create_sql::<M>(chunk);
        let q = bind_sql_values(sqlx::query(&sql), values);
        let result = q.execute(&mut *conn).await
            .with_context(|| format!("bulk_create on {} failed", M::table_name()))?;
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
pub async fn bulk_update<'a, A, M>(executor: A, models: &[M], fields: &[&str]) -> Result<usize>
where
    A: Acquire<'a, Database = Postgres>,
    M: Model + ModelValues,
{
    if models.is_empty() || fields.is_empty() { return Ok(0); }

    let mut conn = executor.acquire().await
        .with_context(|| format!("bulk_update: failed to acquire connection for {}", M::table_name()))?;

    let n_cols = fields.len() + 1; // +1 for pk
    let chunk_size = (65535 / n_cols).max(1);
    let mut total = 0usize;

    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_update_sql::<M>(chunk, fields);
        let q = bind_sql_values(sqlx::query(&sql), values);
        let result = q.execute(&mut *conn).await
            .with_context(|| format!("bulk_update on {} failed", M::table_name()))?;
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
pub async fn bulk_upsert<'a, A, M>(executor: A, models: &[M], conflict_on: &[&str]) -> Result<usize>
where
    A: Acquire<'a, Database = Postgres>,
    M: Model + ModelValues,
{
    if models.is_empty() { return Ok(0); }

    let mut conn = executor.acquire().await
        .with_context(|| format!("bulk_upsert: failed to acquire connection for {}", M::table_name()))?;

    let n_cols = 1 + models[0].field_values().len();
    let chunk_size = (65535 / n_cols).max(1);
    let mut total = 0usize;

    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_upsert_sql::<M>(chunk, conflict_on);
        let q = bind_sql_values(sqlx::query(&sql), values);
        let result = q.execute(&mut *conn).await
            .with_context(|| format!("bulk_upsert on {} failed", M::table_name()))?;
        total += result.rows_affected() as usize;
    }
    Ok(total)
}

// ─── Bulk SQL builders ────────────────────────────────────────────────────────

fn sql_type_cast(val: &SqlValue) -> &'static str {
    match val {
        SqlValue::Null        => "::text",
        SqlValue::Bool(_)     => "::bool",
        SqlValue::SmallInt(_) => "::int2",
        SqlValue::Int(_)      => "::int4",
        SqlValue::BigInt(_)   => "::int8",
        SqlValue::Float(_)    => "::float4",
        SqlValue::Double(_)   => "::float8",
        SqlValue::Text(_)     => "::text",
        SqlValue::Bytes(_)    => "::bytea",
        SqlValue::Uuid(_)     => "::uuid",
        SqlValue::DateTime(_) => "::timestamptz",
        SqlValue::Date(_)     => "::date",
        SqlValue::Time(_)     => "::time",
        SqlValue::Json(_)     => "::jsonb",
    }
}

fn build_bulk_create_sql<M: Model + ModelValues>(models: &[M]) -> (String, Vec<SqlValue>) {
    let pk_col = M::pk_column();
    let sample = models[0].field_values();

    let mut cols = vec![format!("\"{}\"", pk_col)];
    cols.extend(sample.iter().map(|(c, _)| format!("\"{}\"", c)));

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

    let sql = format!(
        "INSERT INTO \"{}\" ({}) VALUES {}",
        M::table_name(), cols.join(", "), row_placeholders.join(", "),
    );
    (sql, all_values)
}

fn build_bulk_update_sql<M: Model + ModelValues>(models: &[M], fields: &[&str]) -> (String, Vec<SqlValue>) {
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
            let val = model_fields.iter()
                .find(|(c, _)| c == f)
                .map(|(_, v)| v.clone())
                .unwrap_or(SqlValue::Null);
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

    let set_clauses: Vec<String> = fields.iter()
        .map(|f| format!("\"{}\" = v.\"{}\"", f, f))
        .collect();

    let alias_cols: Vec<String> = fields.iter()
        .map(|f| format!("\"{}\"", f))
        .chain(std::iter::once(format!("\"{}\"", pk_col)))
        .collect();

    let sql = format!(
        "UPDATE \"{table}\" SET {set} FROM (VALUES {vals}) AS v({alias}) WHERE \"{table}\".\"{pk}\" = v.\"{pk}\"",
        table = table,
        set = set_clauses.join(", "),
        vals = row_placeholders.join(", "),
        alias = alias_cols.join(", "),
        pk = pk_col,
    );
    (sql, all_values)
}

// ─── Raw SQL escape hatch ─────────────────────────────────────────────────────

/// Execute a raw SQL query and return deserialized rows.
///
/// Parameters are positional (`$1`, `$2`, ...) as in PostgreSQL.
///
/// # Example
/// ```rust,ignore
/// let rows = rango::raw::<User>(&pool, "SELECT * FROM users WHERE role = $1", vec![SqlValue::Text("admin".into())]).await?;
/// ```
pub async fn raw<'e, E, M>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<Vec<M>>
where
    E: Executor<'e, Database = Postgres>,
    M: FromRow,
{
    let q = bind_sql_values(sqlx::query(sql), params);
    let rows = q.fetch_all(executor).await
        .with_context(|| format!("raw query failed: {}", sql))?;
    rows.into_iter()
        .map(|r| M::from_row(&PgRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e)))
        .collect()
}

/// Execute a raw SQL query and return a single scalar value (first column of first row).
pub async fn raw_scalar<'e, E, T>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<T>
where
    E: Executor<'e, Database = Postgres>,
    T: for<'r> sqlx::Decode<'r, Postgres> + sqlx::Type<Postgres>,
{
    use sqlx::Row;
    let q = bind_sql_values(sqlx::query(sql), params);
    let row = q.fetch_one(executor).await
        .with_context(|| format!("raw_scalar query failed: {}", sql))?;
    row.try_get::<T, _>(0).map_err(|e| anyhow::anyhow!("raw_scalar decode: {}", e))
}

/// Execute a raw SQL statement (INSERT/UPDATE/DELETE/DDL) — returns rows affected.
pub async fn raw_execute<'e, E>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<u64>
where
    E: Executor<'e, Database = Postgres>,
{
    let q = bind_sql_values(sqlx::query(sql), params);
    q.execute(executor).await
        .map(|r| r.rows_affected())
        .with_context(|| format!("raw_execute failed: {}", sql))
}

fn build_bulk_upsert_sql<M: Model + ModelValues>(models: &[M], conflict_on: &[&str]) -> (String, Vec<SqlValue>) {
    let pk_col = M::pk_column();
    let sample = models[0].field_values();

    let mut cols = vec![format!("\"{}\"", pk_col)];
    cols.extend(sample.iter().map(|(c, _)| format!("\"{}\"", c)));

    let update_cols: Vec<String> = sample.iter()
        .filter(|(c, _)| !conflict_on.contains(c))
        .map(|(c, _)| format!("\"{}\" = EXCLUDED.\"{}\"", c, c))
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

    let conflict_clause = conflict_on.iter()
        .map(|c| format!("\"{}\"", c))
        .collect::<Vec<_>>()
        .join(", ");

    let sql = if update_cols.is_empty() {
        format!(
            "INSERT INTO \"{}\" ({}) VALUES {} ON CONFLICT ({}) DO NOTHING",
            M::table_name(), cols.join(", "), row_placeholders.join(", "), conflict_clause,
        )
    } else {
        format!(
            "INSERT INTO \"{}\" ({}) VALUES {} ON CONFLICT ({}) DO UPDATE SET {}",
            M::table_name(), cols.join(", "), row_placeholders.join(", "), conflict_clause, update_cols.join(", "),
        )
    };
    (sql, all_values)
}
