use rango_core::{FromRow, Model, ModelValues};
use sqlx::PgPool;
use anyhow::{Context, Result};

use crate::ops::bind_sql_values;
use crate::row::PgRangoRow;

/// Many-to-many operations.
/// `From` = the model that declares the M2M field
/// `To`   = the related model
pub struct M2M<From, To> {
    _from: std::marker::PhantomData<From>,
    _to: std::marker::PhantomData<To>,
}

impl<From, To> M2M<From, To>
where
    From: Model + ModelValues,
    To: Model + ModelValues + FromRow,
{
    /// Pivot table name: "{from_table}_{to_table}"
    /// e.g. "geekizz_article_geekizz_tag" or custom via prefix
    fn pivot_table() -> String {
        format!("{}_{}", From::table_name(), To::table_name())
    }

    fn from_col() -> String {
        format!("{}_id", From::table_name())
    }

    fn to_col() -> String {
        format!("{}_id", To::table_name())
    }

    /// Add a relation between from and to.
    /// Uses INSERT OR IGNORE (idempotent).
    pub async fn add(pool: &PgPool, from: &From, to: &To) -> Result<()> {
        let pivot = Self::pivot_table();
        let from_col = Self::from_col();
        let to_col = Self::to_col();
        let sql = format!(
            "INSERT INTO \"{}\" (\"{}\", \"{}\") VALUES ($1, $2) ON CONFLICT DO NOTHING",
            pivot, from_col, to_col
        );
        let q = bind_sql_values(sqlx::query(&sql), vec![
            from.pk_value(),
            to.pk_value(),
        ]);
        q.execute(pool).await
            .with_context(|| format!("M2M add on {} failed", pivot))?;
        Ok(())
    }

    /// Remove a relation.
    pub async fn remove(pool: &PgPool, from: &From, to: &To) -> Result<()> {
        let pivot = Self::pivot_table();
        let from_col = Self::from_col();
        let to_col = Self::to_col();
        let sql = format!(
            "DELETE FROM \"{}\" WHERE \"{}\" = $1 AND \"{}\" = $2",
            pivot, from_col, to_col
        );
        let q = bind_sql_values(sqlx::query(&sql), vec![
            from.pk_value(),
            to.pk_value(),
        ]);
        q.execute(pool).await
            .with_context(|| format!("M2M remove on {} failed", pivot))?;
        Ok(())
    }

    /// Clear all relations from a given model instance.
    pub async fn clear(pool: &PgPool, from: &From) -> Result<()> {
        let pivot = Self::pivot_table();
        let from_col = Self::from_col();
        let sql = format!("DELETE FROM \"{}\" WHERE \"{}\" = $1", pivot, from_col);
        let q = bind_sql_values(sqlx::query(&sql), vec![from.pk_value()]);
        q.execute(pool).await
            .with_context(|| format!("M2M clear on {} failed", pivot))?;
        Ok(())
    }

    /// Get all related `To` models for a given `from` instance.
    pub async fn all(pool: &PgPool, from: &From) -> Result<Vec<To>> {
        let pivot = Self::pivot_table();
        let from_col = Self::from_col();
        let to_table = To::table_name();
        let to_pk = To::pk_column();
        let to_col = Self::to_col();

        let sql = format!(
            r#"SELECT t.* FROM "{to_table}" t
               INNER JOIN "{pivot}" p ON t."{to_pk}" = p."{to_col}"
               WHERE p."{from_col}" = $1"#,
        );
        let q = bind_sql_values(sqlx::query(&sql), vec![from.pk_value()]);
        let rows = q.fetch_all(pool).await
            .with_context(|| format!("M2M all on {} failed", pivot))?;

        rows.into_iter()
            .map(|r| To::from_row(&PgRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e)))
            .collect()
    }

    /// Get all `From` models related to a given `to` instance (reverse).
    pub async fn reverse(pool: &PgPool, to: &To) -> Result<Vec<From>>
    where
        From: FromRow,
    {
        let pivot = Self::pivot_table();
        let to_col = Self::to_col();
        let from_table = From::table_name();
        let from_pk = From::pk_column();
        let from_col = Self::from_col();

        let sql = format!(
            r#"SELECT f.* FROM "{from_table}" f
               INNER JOIN "{pivot}" p ON f."{from_pk}" = p."{from_col}"
               WHERE p."{to_col}" = $1"#,
        );
        let q = bind_sql_values(sqlx::query(&sql), vec![to.pk_value()]);
        let rows = q.fetch_all(pool).await
            .with_context(|| format!("M2M reverse on {} failed", pivot))?;

        rows.into_iter()
            .map(|r| From::from_row(&PgRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e)))
            .collect()
    }

    /// Set exact relations — replaces all existing with the given list.
    pub async fn set(pool: &PgPool, from: &From, to_list: &[To]) -> Result<()> {
        Self::clear(pool, from).await?;
        for to in to_list {
            Self::add(pool, from, to).await?;
        }
        Ok(())
    }

    /// Check if a relation exists.
    pub async fn exists(pool: &PgPool, from: &From, to: &To) -> Result<bool> {
        let pivot = Self::pivot_table();
        let from_col = Self::from_col();
        let to_col = Self::to_col();
        let sql = format!(
            "SELECT 1 FROM \"{}\" WHERE \"{}\" = $1 AND \"{}\" = $2 LIMIT 1",
            pivot, from_col, to_col
        );
        let q = bind_sql_values(sqlx::query(&sql), vec![from.pk_value(), to.pk_value()]);
        let row = q.fetch_optional(pool).await
            .with_context(|| format!("M2M exists on {} failed", pivot))?;
        Ok(row.is_some())
    }
}
