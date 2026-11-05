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
    let n_cols = 1 + models[0].field_values().len();
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

fn build_bulk_create_sql<M: Model + ModelValues>(models: &[M]) -> (String, Vec<SqlValue>) {
    let pk_col = M::pk_column();
    let sample = models[0].field_values();
    let mut cols = vec![quote_ident(pk_col)];
    cols.extend(sample.iter().map(|(c, _)| quote_ident(c)));
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
        "INSERT INTO {} ({}) VALUES {}",
        quote_ident(M::table_name()),
        cols.join(", "),
        row_placeholders.join(", ")
    );
    (sql, all_values)
}
