use crate::error::db_err;
use rango_core::{Error, Result};
/// Postgres-specific query extensions.
///
/// Import this trait to unlock Postgres-only filter methods on any `QueryBuilder`.
/// These methods generate SQL that is not portable to other backends.
///
/// # Example
/// ```rust,ignore
/// use rango_postgres::PgQueryExt;
///
/// let articles = Article::filter(&pool)
///     .fts("content", "rust ORM")
///     .json_contains("metadata", serde_json::json!({"status": "published"}))
///     .all()
///     .await?;
/// ```
use rango_core::{ColumnRef, FromRow, Model, ModelValues, SqlValue, quote_ident};
use sqlx::{PgPool, Row};

use crate::ops::bind_sql_values;
use crate::query_builder::QueryBuilder;
use crate::related::WithRelated;
use crate::row::PgRangoRow;

/// Postgres-only extensions for `QueryBuilder`.
pub trait PgQueryExt<M>
where
    M: Model + ModelValues + FromRow,
{
    /// Filter by full-text search using PostgreSQL `to_tsvector` / `plainto_tsquery`.
    ///
    /// ```sql
    /// WHERE to_tsvector('english', "col") @@ plainto_tsquery('english', 'query')
    /// ```
    fn fts(self, col: impl ColumnRef<M>, query: &str) -> Self;

    /// Filter by full-text search with a custom language config.
    fn fts_lang(self, col: impl ColumnRef<M>, query: &str, lang: &str) -> Self;

    /// Filter rows where a JSONB column contains the given value.
    ///
    /// ```sql
    /// WHERE "col" @> '{"key": "value"}'
    /// ```
    fn json_contains(self, col: impl ColumnRef<M>, value: serde_json::Value) -> Self;

    /// Filter rows where a JSONB path exists.
    ///
    /// ```sql
    /// WHERE "col" ? 'key'
    /// ```
    fn json_has_key(self, col: impl ColumnRef<M>, key: &str) -> Self;

    /// Filter using a raw JSONB expression.
    ///
    /// ```sql
    /// WHERE "col"->>'key' = 'value'
    /// ```
    fn json_field_eq(self, col: impl ColumnRef<M>, key: &str, value: &str) -> Self;

    /// Filter by array containment: column contains all given values.
    ///
    /// ```sql
    /// WHERE "col" @> ARRAY['a', 'b']
    /// ```
    fn array_contains(self, col: impl ColumnRef<M>, values: Vec<String>) -> Self;
}

impl<M> PgQueryExt<M> for QueryBuilder<M>
where
    M: Model + ModelValues + FromRow,
{
    fn fts(self, col: impl ColumnRef<M>, query: &str) -> Self {
        self.raw_where(
            format!(
                "to_tsvector('english', {}) @@ plainto_tsquery('english', $__)",
                quote_ident(col.column_name())
            ),
            vec![SqlValue::Text(query.to_string())],
        )
    }

    fn fts_lang(self, col: impl ColumnRef<M>, query: &str, lang: &str) -> Self {
        self.raw_where(
            format!(
                "to_tsvector($__::regconfig, {}) @@ plainto_tsquery($__::regconfig, $__)",
                quote_ident(col.column_name())
            ),
            vec![
                SqlValue::Text(lang.to_string()),
                SqlValue::Text(lang.to_string()),
                SqlValue::Text(query.to_string()),
            ],
        )
    }

    fn json_contains(self, col: impl ColumnRef<M>, value: serde_json::Value) -> Self {
        self.raw_where(
            format!("{} @> $__::jsonb", quote_ident(col.column_name())),
            vec![SqlValue::Json(value)],
        )
    }

    fn json_has_key(self, col: impl ColumnRef<M>, key: &str) -> Self {
        self.raw_where(
            format!("{} ? $__", quote_ident(col.column_name())),
            vec![SqlValue::Text(key.to_string())],
        )
    }

    fn json_field_eq(self, col: impl ColumnRef<M>, key: &str, value: &str) -> Self {
        self.raw_where(
            format!("{}->>$__ = $__", quote_ident(col.column_name())),
            vec![
                SqlValue::Text(key.to_string()),
                SqlValue::Text(value.to_string()),
            ],
        )
    }

    fn array_contains(self, col: impl ColumnRef<M>, values: Vec<String>) -> Self {
        self.raw_where(
            format!(
                "{} @> ARRAY(SELECT jsonb_array_elements_text($__::jsonb))",
                quote_ident(col.column_name())
            ),
            vec![SqlValue::Json(serde_json::Value::from(values))],
        )
    }
}

/// Standalone Postgres-specific query functions (not on QueryBuilder).
pub struct Pg;

impl Pg {
    /// Execute a full-text search and return matching rows.
    pub async fn fts<M>(pool: &PgPool, col: impl ColumnRef<M>, query: &str) -> Result<Vec<WithRelated<M>>>
    where
        M: Model + ModelValues + FromRow,
    {
        use crate::ops::DEFAULT_QUERY_LIMIT;
        let sql = format!(
            "SELECT * FROM {} WHERE to_tsvector('english', {}) @@ plainto_tsquery('english', $1) LIMIT {}",
            quote_ident(M::table_name()),
            quote_ident(col.column_name()),
            DEFAULT_QUERY_LIMIT
        );
        let rows = bind_sql_values(sqlx::query(&sql), vec![SqlValue::Text(query.to_string())])
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                M::from_row(&PgRangoRow(r))
                    .map(WithRelated::new)
                    .map_err(Error::from)
            })
            .collect()
    }

    /// Get a JSONB field value as a string.
    pub async fn json_get<M>(
        pool: &PgPool,
        pk: &SqlValue,
        col: impl ColumnRef<M>,
        key: &str,
    ) -> Result<Option<String>>
    where
        M: Model + ModelValues,
    {
        let sql = format!(
            "SELECT {}->>$1 FROM {} WHERE {} = $2",
            quote_ident(col.column_name()),
            quote_ident(M::table_name()),
            quote_ident(M::pk_column())
        );
        let row = bind_sql_values(
            sqlx::query(&sql),
            vec![SqlValue::Text(key.to_string()), pk.clone()],
        )
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
        Ok(row.and_then(|r| r.try_get::<Option<String>, _>(0).ok().flatten()))
    }
}
