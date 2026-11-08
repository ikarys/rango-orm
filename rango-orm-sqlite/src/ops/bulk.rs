//! Multi-row operations: bulk insert, update and upsert.

use crate::error::db_err;
use rango_orm_core::Result;
use rango_orm_core::{quote_ident, Model, ModelValues, SqlValue};
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
    let mut total = 0usize;
    // One transaction for all chunks (a savepoint when `executor` is already a transaction):
    // a failure in a later chunk must not leave the earlier rows behind.
    let mut tx = executor.begin().await.map_err(db_err)?;
    for chunk in models.chunks(chunk_size) {
        let (sql, values) = build_bulk_create_sql::<M>(chunk);
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(&*sql)), values);
        let result = q.execute(&mut *tx).await.map_err(db_err)?;
        total += result.rows_affected() as usize;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(total)
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
