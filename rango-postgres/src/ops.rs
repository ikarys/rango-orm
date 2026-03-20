use rango_core::{FromRow, Model, ModelValues, SqlValue};
use sqlx::PgPool;
use anyhow::{Context, Result};

use crate::row::PgRangoRow;

/// Bind a SqlValue to a sqlx query.
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

/// INSERT → returns the model as stored.
pub async fn insert<M>(pool: &PgPool, model: M) -> Result<M>
where
    M: Model + ModelValues + FromRow,
{
    let table = M::table_name();
    let fields = model.field_values();
    let pk_col = M::pk_column();
    let pk_val = model.pk_value();

    let mut cols = vec![format!("\"{}\"", pk_col)];
    cols.extend(fields.iter().map(|(c, _)| format!("\"{}\"", c)));
    let placeholders: Vec<String> = (1..=cols.len()).map(|i| format!("${}", i)).collect();

    let sql = format!(
        "INSERT INTO \"{}\" ({}) VALUES ({}) RETURNING *",
        table,
        cols.join(", "),
        placeholders.join(", ")
    );

    let mut all_values = vec![pk_val];
    all_values.extend(fields.into_iter().map(|(_, v)| v));

    let mut q = sqlx::query(&sql);
    for val in all_values {
        q = bind!(q, val);
    }

    let row = q.fetch_one(pool)
        .await
        .with_context(|| format!("INSERT into {} failed", table))?;

    M::from_row(&PgRangoRow(row))
        .map_err(|e| anyhow::anyhow!("Failed to read inserted row: {}", e))
}

/// UPDATE → returns the updated model.
pub async fn update<M>(pool: &PgPool, model: M) -> Result<M>
where
    M: Model + ModelValues + FromRow,
{
    let table = M::table_name();
    let fields = model.field_values();
    let pk_col = M::pk_column();
    let pk_val = model.pk_value();

    if fields.is_empty() {
        return Ok(model);
    }

    let set_clauses: Vec<String> = fields.iter().enumerate()
        .map(|(i, (col, _))| format!("\"{}\" = ${}", col, i + 1))
        .collect();

    let pk_pos = fields.len() + 1;
    let sql = format!(
        "UPDATE \"{}\" SET {} WHERE \"{}\" = ${} RETURNING *",
        table, set_clauses.join(", "), pk_col, pk_pos
    );

    let mut q = sqlx::query(&sql);
    for (_, val) in &fields {
        q = bind!(q, val.clone());
    }
    q = bind!(q, pk_val);

    let row = q.fetch_one(pool)
        .await
        .with_context(|| format!("UPDATE {} failed", table))?;

    M::from_row(&PgRangoRow(row))
        .map_err(|e| anyhow::anyhow!("Failed to read updated row: {}", e))
}

/// DELETE a model.
pub async fn delete<M>(pool: &PgPool, model: &M) -> Result<()>
where
    M: Model + ModelValues,
{
    let table = M::table_name();
    let pk_col = M::pk_column();
    let pk_val = model.pk_value();

    let sql = format!("DELETE FROM \"{}\" WHERE \"{}\" = $1", table, pk_col);
    let q = sqlx::query(&sql);
    let q = bind!(q, pk_val);
    q.execute(pool).await
        .with_context(|| format!("DELETE from {} failed", table))?;
    Ok(())
}

/// GET by primary key.
pub async fn get<M>(pool: &PgPool, pk: &SqlValue) -> Result<Option<M>>
where
    M: Model + ModelValues + FromRow,
{
    let table = M::table_name();
    let pk_col = M::pk_column();
    let sql = format!("SELECT * FROM \"{}\" WHERE \"{}\" = $1 LIMIT 1", table, pk_col);

    let q = sqlx::query(&sql);
    let q = bind!(q, pk.clone());

    let row = q.fetch_optional(pool).await
        .with_context(|| format!("GET from {} failed", table))?;

    match row {
        Some(r) => Ok(Some(M::from_row(&PgRangoRow(r))
            .map_err(|e| anyhow::anyhow!("{}", e))?)),
        None => Ok(None),
    }
}

/// GET ALL rows.
pub async fn all<M>(pool: &PgPool) -> Result<Vec<M>>
where
    M: Model + ModelValues + FromRow,
{
    let table = M::table_name();
    let sql = format!("SELECT * FROM \"{}\"", table);

    let rows = sqlx::query(&sql).fetch_all(pool).await
        .with_context(|| format!("ALL from {} failed", table))?;

    rows.into_iter()
        .map(|r| M::from_row(&PgRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e)))
        .collect()
}

/// GET OR CREATE — returns (model, created: bool).
pub async fn get_or_create<M>(
    pool: &PgPool,
    lookup: Vec<(&'static str, SqlValue)>,
    defaults: M,
) -> Result<(M, bool)>
where
    M: Model + ModelValues + FromRow,
{
    let table = M::table_name();
    let conditions: Vec<String> = lookup.iter().enumerate()
        .map(|(i, (col, _))| format!("\"{}\" = ${}", col, i + 1))
        .collect();

    let sql = format!(
        "SELECT * FROM \"{}\" WHERE {} LIMIT 1",
        table,
        conditions.join(" AND ")
    );

    let mut q = sqlx::query(&sql);
    for (_, val) in &lookup {
        q = bind!(q, val.clone());
    }

    let row = q.fetch_optional(pool).await
        .with_context(|| format!("GET_OR_CREATE lookup on {} failed", table))?;

    if let Some(r) = row {
        let model = M::from_row(&PgRangoRow(r))
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        return Ok((model, false));
    }

    let created = insert(pool, defaults).await?;
    Ok((created, true))
}

/// UPDATE OR CREATE — updates if found, creates if not. Returns the model.
pub async fn update_or_create<M>(
    pool: &PgPool,
    lookup: Vec<(&'static str, SqlValue)>,
    values: M,
) -> Result<M>
where
    M: Model + ModelValues + FromRow,
{
    let table = M::table_name();
    let conditions: Vec<String> = lookup.iter().enumerate()
        .map(|(i, (col, _))| format!("\"{}\" = ${}", col, i + 1))
        .collect();

    let sql = format!(
        "SELECT * FROM \"{}\" WHERE {} LIMIT 1",
        table,
        conditions.join(" AND ")
    );

    let mut q = sqlx::query(&sql);
    for (_, val) in &lookup {
        q = bind!(q, val.clone());
    }

    let row = q.fetch_optional(pool).await
        .with_context(|| format!("UPDATE_OR_CREATE lookup on {} failed", table))?;

    if row.is_some() {
        update(pool, values).await
    } else {
        insert(pool, values).await
    }
}
