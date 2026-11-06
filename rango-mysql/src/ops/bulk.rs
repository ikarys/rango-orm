//! Multi-row operations: bulk insert, update and upsert.

use crate::error::db_err;
use rango_core::Result;
use rango_core::{quote_ident_mysql as quote_ident, Model, ModelValues, SqlValue};
use sqlx::MySqlPool;

use super::*;

pub async fn bulk_create<M>(pool: &MySqlPool, models: &[M]) -> Result<usize>
where
    M: Model + ModelValues,
{
    if models.is_empty() {
        return Ok(0);
    }
    let n_cols = rango_core::insert_values(&models[0]).len();
    let chunk_size = (65_535 / n_cols).max(1); // MySQL limit: 65 535 placeholders per statement
    let mut total = 0usize;
    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_create_sql::<M>(chunk);
        let q = bind_sql_values(sqlx::query(&sql), values);
        let result = q.execute(pool).await.map_err(db_err)?;
        total += result.rows_affected() as usize;
    }
    Ok(total)
}

/// Columns come from the first model: give every model a PK, or none (auto-increment).
fn build_bulk_create_sql<M: Model + ModelValues>(models: &[M]) -> (String, Vec<SqlValue>) {
    let cols: Vec<String> = rango_core::insert_values(&models[0])
        .into_iter()
        .map(|(c, _)| quote_ident(c))
        .collect();
    let mut all_values: Vec<SqlValue> = Vec::new();
    let mut row_placeholders: Vec<String> = Vec::new();
    for model in models {
        let values = rango_core::insert_values(model);
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
