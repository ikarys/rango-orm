use anyhow::{Context, Result};
use rango_core::{FromRow, Model, ModelValues, SqlValue};
use sqlx::query::Query;
use sqlx::sqlite::SqliteArguments;
use sqlx::{Executor, Sqlite, SqlitePool};

use crate::row::SqliteRangoRow;

// ─── Bind macro ───────────────────────────────────────────────────────────────

macro_rules! bind {
    ($q:expr, $val:expr) => {
        match $val {
            // Typed nulls
            SqlValue::NullBool => $q.bind(Option::<i64>::None),
            SqlValue::NullSmallInt => $q.bind(Option::<i64>::None),
            SqlValue::NullInt => $q.bind(Option::<i64>::None),
            SqlValue::NullBigInt => $q.bind(Option::<i64>::None),
            SqlValue::NullFloat => $q.bind(Option::<f64>::None),
            SqlValue::NullDouble => $q.bind(Option::<f64>::None),
            SqlValue::NullText => $q.bind(Option::<String>::None),
            SqlValue::NullBytes => $q.bind(Option::<Vec<u8>>::None),
            SqlValue::NullUuid => $q.bind(Option::<String>::None),
            SqlValue::NullDateTime => $q.bind(Option::<String>::None),
            SqlValue::NullDate => $q.bind(Option::<String>::None),
            SqlValue::NullTime => $q.bind(Option::<String>::None),
            SqlValue::NullJson => $q.bind(Option::<String>::None),
            // Values
            SqlValue::Bool(v) => $q.bind(v as i64),
            SqlValue::SmallInt(v) => $q.bind(v as i64),
            SqlValue::Int(v) => $q.bind(v as i64),
            SqlValue::BigInt(v) => $q.bind(v),
            SqlValue::Float(v) => $q.bind(v as f64),
            SqlValue::Double(v) => $q.bind(v),
            SqlValue::Text(v) => $q.bind(v),
            SqlValue::Bytes(v) => $q.bind(v),
            SqlValue::Uuid(v) => $q.bind(v.to_string()),
            SqlValue::DateTime(v) => $q.bind(v.to_rfc3339()),
            SqlValue::Date(v) => $q.bind(v.to_string()),
            SqlValue::Time(v) => $q.bind(v.to_string()),
            SqlValue::Json(v) => $q.bind(v.to_string()),
        }
    };
}

pub fn bind_sql_values<'q>(
    mut q: Query<'q, Sqlite, SqliteArguments<'q>>,
    values: Vec<SqlValue>,
) -> Query<'q, Sqlite, SqliteArguments<'q>> {
    for val in values {
        q = bind!(q, val);
    }
    q
}

// ─── CRUD ops ─────────────────────────────────────────────────────────────────

pub async fn insert<'e, E, M>(executor: E, model: M) -> Result<M>
where
    E: Executor<'e, Database = Sqlite>,
    M: Model + ModelValues + FromRow,
{
    let (sql, values) = build_insert_sql::<M>(&model);
    let q = bind_sql_values(sqlx::query(&sql), values);
    let row = q
        .fetch_one(executor)
        .await
        .with_context(|| format!("INSERT into {} failed", M::table_name()))?;
    M::from_row(&SqliteRangoRow(row)).map_err(|e| anyhow::anyhow!("{}", e))
}

pub async fn update<'e, E, M>(executor: E, model: M) -> Result<M>
where
    E: Executor<'e, Database = Sqlite>,
    M: Model + ModelValues + FromRow,
{
    let (sql, values) = build_update_sql::<M>(&model);
    let q = bind_sql_values(sqlx::query(&sql), values);
    let row = q
        .fetch_one(executor)
        .await
        .with_context(|| format!("UPDATE {} failed", M::table_name()))?;
    M::from_row(&SqliteRangoRow(row)).map_err(|e| anyhow::anyhow!("{}", e))
}

pub async fn delete<'e, E, M>(executor: E, model: &M) -> Result<()>
where
    E: Executor<'e, Database = Sqlite>,
    M: Model + ModelValues,
{
    let sql = format!(
        "DELETE FROM \"{}\" WHERE \"{}\" = ?",
        M::table_name(),
        M::pk_column()
    );
    let q = bind_sql_values(sqlx::query(&sql), vec![model.pk_value()]);
    q.execute(executor)
        .await
        .map(|_| ())
        .with_context(|| format!("DELETE from {} failed", M::table_name()))
}

pub async fn get<'e, E, M>(executor: E, pk: &SqlValue) -> Result<Option<M>>
where
    E: Executor<'e, Database = Sqlite>,
    M: Model + ModelValues + FromRow,
{
    let sql = format!(
        "SELECT * FROM \"{}\" WHERE \"{}\" = ? LIMIT 1",
        M::table_name(),
        M::pk_column()
    );
    let q = bind_sql_values(sqlx::query(&sql), vec![pk.clone()]);
    let row = q
        .fetch_optional(executor)
        .await
        .with_context(|| format!("GET from {} failed", M::table_name()))?;
    match row {
        Some(r) => Ok(Some(
            M::from_row(&SqliteRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e))?,
        )),
        None => Ok(None),
    }
}

pub async fn all<'e, E, M>(executor: E) -> Result<Vec<M>>
where
    E: Executor<'e, Database = Sqlite>,
    M: Model + ModelValues + FromRow,
{
    let sql = format!("SELECT * FROM \"{}\"", M::table_name());
    let rows = sqlx::query(&sql)
        .fetch_all(executor)
        .await
        .with_context(|| format!("ALL from {} failed", M::table_name()))?;
    rows.into_iter()
        .map(|r| M::from_row(&SqliteRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e)))
        .collect()
}

pub async fn get_or_create<M>(
    pool: &SqlitePool,
    lookup: Vec<(&'static str, SqlValue)>,
    defaults: M,
) -> Result<(M, bool)>
where
    M: Model + ModelValues + FromRow,
{
    let conflict_cols: Vec<&str> = lookup.iter().map(|(col, _)| *col).collect();
    let (insert_sql, insert_vals) = build_get_or_create_sql(&defaults, &conflict_cols);
    let q = bind_sql_values(sqlx::query(&insert_sql), insert_vals);
    let row = q
        .fetch_optional(pool)
        .await
        .with_context(|| format!("get_or_create INSERT on {} failed", M::table_name()))?;

    if let Some(r) = row {
        return Ok((
            M::from_row(&SqliteRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e))?,
            true,
        ));
    }

    let (select_sql, select_vals) = build_lookup_sql::<M>(&lookup);
    let q = bind_sql_values(sqlx::query(&select_sql), select_vals);
    let row = q
        .fetch_one(pool)
        .await
        .with_context(|| format!("get_or_create SELECT on {} failed", M::table_name()))?;
    Ok((
        M::from_row(&SqliteRangoRow(row)).map_err(|e| anyhow::anyhow!("{}", e))?,
        false,
    ))
}

pub async fn bulk_create<M>(pool: &SqlitePool, models: &[M]) -> Result<usize>
where
    M: Model + ModelValues,
{
    if models.is_empty() {
        return Ok(0);
    }
    let n_cols = 1 + models[0].field_values().len();
    let chunk_size = (999 / n_cols).max(1); // SQLite limit: 999 bound params
    let mut total = 0usize;
    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_create_sql::<M>(chunk);
        let q = bind_sql_values(sqlx::query(&sql), values);
        let result = q
            .execute(pool)
            .await
            .with_context(|| format!("bulk_create on {} failed", M::table_name()))?;
        total += result.rows_affected() as usize;
    }
    Ok(total)
}

/// Raw SQL — returns deserialized model rows.
pub async fn raw<'e, E, M>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<Vec<M>>
where
    E: Executor<'e, Database = Sqlite>,
    M: FromRow,
{
    let q = bind_sql_values(sqlx::query(sql), params);
    let rows = q
        .fetch_all(executor)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    rows.into_iter()
        .map(|r| M::from_row(&SqliteRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e)))
        .collect()
}

/// Raw SQL — returns a single scalar value.
pub async fn raw_scalar<'e, E, T>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<T>
where
    E: Executor<'e, Database = Sqlite>,
    T: for<'r> sqlx::Decode<'r, Sqlite> + sqlx::Type<Sqlite>,
{
    use sqlx::Row;
    let q = bind_sql_values(sqlx::query(sql), params);
    let row = q
        .fetch_one(executor)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    row.try_get::<T, _>(0)
        .map_err(|e| anyhow::anyhow!("raw_scalar decode: {}", e))
}

/// Raw SQL — execute statement, return rows affected.
pub async fn raw_execute<'e, E>(executor: E, sql: &str, params: Vec<SqlValue>) -> Result<u64>
where
    E: Executor<'e, Database = Sqlite>,
{
    let q = bind_sql_values(sqlx::query(sql), params);
    q.execute(executor)
        .await
        .map(|r| r.rows_affected())
        .with_context(|| format!("raw_execute failed: {}", sql))
}

// ─── SQL builders ─────────────────────────────────────────────────────────────

fn build_insert_sql<M: Model + ModelValues>(model: &M) -> (String, Vec<SqlValue>) {
    let fields = model.field_values();
    let pk_col = M::pk_column();
    let pk_val = model.pk_value();
    let mut cols = vec![format!("\"{}\"", pk_col)];
    cols.extend(fields.iter().map(|(c, _)| format!("\"{}\"", c)));
    let placeholders: Vec<String> = (0..cols.len()).map(|_| "?".to_string()).collect();
    let sql = format!(
        "INSERT INTO \"{}\" ({}) VALUES ({}) RETURNING *",
        M::table_name(),
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
        .map(|(col, _)| format!("\"{}\" = ?", col))
        .collect();
    let sql = format!(
        "UPDATE \"{}\" SET {} WHERE \"{}\" = ? RETURNING *",
        M::table_name(),
        set_clauses.join(", "),
        pk_col
    );
    let mut values: Vec<SqlValue> = fields.into_iter().map(|(_, v)| v).collect();
    values.push(pk_val);
    (sql, values)
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
    let placeholders: Vec<String> = (0..cols.len()).map(|_| "?".to_string()).collect();
    let conflict_clause = conflict_cols
        .iter()
        .map(|c| format!("\"{}\"", c))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT OR IGNORE INTO \"{}\" ({}) VALUES ({}) RETURNING *",
        M::table_name(),
        cols.join(", "),
        placeholders.join(", ")
    );
    // Note: SQLite INSERT OR IGNORE doesn't return rows on conflict, so we fall back to SELECT
    let _ = conflict_clause; // used implicitly via OR IGNORE
    let mut values = vec![pk_val];
    values.extend(fields.into_iter().map(|(_, v)| v));
    (sql, values)
}

fn build_lookup_sql<M: Model + ModelValues>(
    lookup: &[(&'static str, SqlValue)],
) -> (String, Vec<SqlValue>) {
    let conditions: Vec<String> = lookup
        .iter()
        .map(|(col, _)| format!("\"{}\" = ?", col))
        .collect();
    let sql = format!(
        "SELECT * FROM \"{}\" WHERE {} LIMIT 1",
        M::table_name(),
        conditions.join(" AND ")
    );
    let values = lookup.iter().map(|(_, v)| v.clone()).collect();
    (sql, values)
}

fn build_bulk_create_sql<M: Model + ModelValues>(models: &[M]) -> (String, Vec<SqlValue>) {
    let pk_col = M::pk_column();
    let sample = models[0].field_values();
    let mut cols = vec![format!("\"{}\"", pk_col)];
    cols.extend(sample.iter().map(|(c, _)| format!("\"{}\"", c)));
    let mut all_values: Vec<SqlValue> = Vec::new();
    let mut row_placeholders: Vec<String> = Vec::new();
    for model in models {
        let fields = model.field_values();
        let pk_val = model.pk_value();
        let ph: Vec<String> = (0..=fields.len()).map(|_| "?".to_string()).collect();
        all_values.push(pk_val);
        all_values.extend(fields.into_iter().map(|(_, v)| v));
        row_placeholders.push(format!("({})", ph.join(", ")));
    }
    let sql = format!(
        "INSERT INTO \"{}\" ({}) VALUES {}",
        M::table_name(),
        cols.join(", "),
        row_placeholders.join(", ")
    );
    (sql, all_values)
}
