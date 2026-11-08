//! Multi-row operations: bulk insert, update and upsert.

use crate::error::db_err;
use rango_orm_core::Result;
use rango_orm_core::{Model, ModelValues, SqlValue, quote_ident};
use sqlx::postgres::PgArguments;
use sqlx::query::Query;
use sqlx::{Acquire, Postgres};

use super::*;

// ─── Bulk ops ─────────────────────────────────────────────────────────────────
/// INSERT multiple rows in a single query.
///
/// Uses `INSERT … SELECT FROM UNNEST(…)`: one array parameter per column, so the
/// 65 535 bind-parameter limit does not apply (JSON columns use chunked multi-row
/// `VALUES` instead). Returns the total number of rows inserted.
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

    // One statement, one array parameter per column; falls back for unsupported types.
    if let Some((sql, cols)) = build_bulk_create_unnest_sql::<M>(models) {
        let q = cols
            .into_iter()
            .fold(sqlx::query(sqlx::AssertSqlSafe(&*sql)), |q, c| c.bind(q));
        let result = q.execute(&mut *conn).await.map_err(db_err)?;
        return Ok(result.rows_affected() as usize);
    }

    let n_cols = rango_orm_core::insert_values(&models[0]).len();
    let chunk_size = (65535 / n_cols).max(1);
    let mut total = 0usize;
    // All chunks in one transaction: a later failure must not leave earlier rows behind.
    let mut tx = (&mut *conn).begin().await.map_err(db_err)?;

    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_create_sql::<M>(chunk);
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), values);
        let result = q.execute(&mut *tx).await.map_err(db_err)?;
        total += result.rows_affected() as usize;
    }
    tx.commit().await.map_err(db_err)?;
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
    let mut tx = (&mut *conn).begin().await.map_err(db_err)?;

    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_update_sql::<M>(chunk, fields);
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), values);
        let result = q.execute(&mut *tx).await.map_err(db_err)?;
        total += result.rows_affected() as usize;
    }
    tx.commit().await.map_err(db_err)?;
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
    let mut tx = (&mut *conn).begin().await.map_err(db_err)?;

    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_upsert_sql::<M>(chunk, conflict_on);
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), values);
        let result = q.execute(&mut *tx).await.map_err(db_err)?;
        total += result.rows_affected() as usize;
    }
    tx.commit().await.map_err(db_err)?;
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
        | SqlValue::NullDecimal
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
        SqlValue::Decimal(_) => "::numeric",
        SqlValue::Text(_) => "::text",
        SqlValue::Bytes(_) => "::bytea",
        SqlValue::Uuid(_) => "::uuid",
        SqlValue::DateTime(_) => "::timestamptz",
        SqlValue::Date(_) => "::date",
        SqlValue::Time(_) => "::time",
        SqlValue::Json(_) => "::jsonb",
    }
}

/// One column of a bulk insert, as a typed array bound to a single `UNNEST` parameter.
macro_rules! unnest_cols {
    ($($var:ident, $null:ident, $ty:ty, $pg:literal);* $(;)?) => {
        enum UnnestCol { $($var(Vec<Option<$ty>>)),* }

        impl UnnestCol {
            /// `None` for value types without an array mapping (JSON).
            fn for_value(v: &SqlValue, cap: usize) -> Option<Self> {
                match v {
                    $(SqlValue::$var(_) | SqlValue::$null => Some(Self::$var(Vec::with_capacity(cap))),)*
                    _ => None,
                }
            }

            fn pg_type(&self) -> &'static str {
                match self { $(Self::$var(_) => $pg),* }
            }

            /// `false` when the value does not match the column type.
            fn push(&mut self, v: SqlValue) -> bool {
                match (self, v) {
                    $(
                        (Self::$var(c), SqlValue::$var(x)) => c.push(Some(x)),
                        (Self::$var(c), SqlValue::$null) => c.push(None),
                    )*
                    _ => return false,
                }
                true
            }

            fn bind<'q>(self, q: Query<'q, Postgres, PgArguments>) -> Query<'q, Postgres, PgArguments> {
                match self { $(Self::$var(c) => q.bind(c)),* }
            }
        }
    };
}

unnest_cols! {
    Bool, NullBool, bool, "bool[]";
    SmallInt, NullSmallInt, i16, "int2[]";
    Int, NullInt, i32, "int4[]";
    BigInt, NullBigInt, i64, "int8[]";
    Float, NullFloat, f32, "float4[]";
    Double, NullDouble, f64, "float8[]";
    Decimal, NullDecimal, rango_orm_core::Decimal, "numeric[]";
    Text, NullText, String, "text[]";
    Bytes, NullBytes, Vec<u8>, "bytea[]";
    Uuid, NullUuid, uuid::Uuid, "uuid[]";
    DateTime, NullDateTime, chrono::DateTime<chrono::Utc>, "timestamptz[]";
    Date, NullDate, chrono::NaiveDate, "date[]";
    Time, NullTime, chrono::NaiveTime, "time[]";
}

fn unnest_sql(table: &str, cols: &[(&str, &str)]) -> String {
    let names: Vec<String> = cols.iter().map(|(c, _)| quote_ident(c)).collect();
    let arrays: Vec<String> = cols
        .iter()
        .enumerate()
        .map(|(i, (_, ty))| format!("${}::{}", i + 1, ty))
        .collect();
    format!(
        "INSERT INTO {} ({}) SELECT * FROM UNNEST({})",
        quote_ident(table),
        names.join(", "),
        arrays.join(", "),
    )
}

/// `None` when a column has no array mapping: the caller uses the multi-row `VALUES` path.
fn build_bulk_create_unnest_sql<M: Model + ModelValues>(
    models: &[M],
) -> Option<(String, Vec<UnnestCol>)> {
    let first = rango_orm_core::insert_values(&models[0]);
    let mut cols = Vec::with_capacity(first.len());
    for (_, v) in &first {
        cols.push(UnnestCol::for_value(v, models.len())?);
    }
    for model in models {
        for (col, (_, v)) in cols.iter_mut().zip(rango_orm_core::insert_values(model)) {
            if !col.push(v) {
                return None;
            }
        }
    }
    let spec: Vec<(&str, &str)> = first
        .iter()
        .zip(&cols)
        .map(|((name, _), c)| (*name, c.pg_type()))
        .collect();
    Some((unnest_sql(M::table_name(), &spec), cols))
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

#[cfg(test)]
mod tests {
    use super::unnest_sql;

    #[test]
    fn unnest_sql_one_array_per_column() {
        assert_eq!(
            unnest_sql("user", &[("id", "uuid[]"), ("sc\"ore", "int4[]")]),
            r#"INSERT INTO "user" ("id", "sc""ore") SELECT * FROM UNNEST($1::uuid[], $2::int4[])"#
        );
    }
}
