/// SQLite-specific query extensions.
///
/// Import this trait to unlock SQLite-only filter methods on any `QueryBuilder`.
/// These methods generate SQL that is specific to SQLite and not portable to other backends.
///
/// # Example
/// ```rust,ignore
/// use rango_sqlite::SqliteQueryExt;
///
/// let posts = Post::filter(&pool)
///     .json_extract_eq("metadata", "$.status", "published")
///     .all()
///     .await?;
/// ```
use rango_core::{FromRow, Model, ModelValues, SqlValue};

use crate::query_builder::QueryBuilder;

pub trait SqliteQueryExt<M>
where
    M: Model + ModelValues + FromRow,
{
    /// Filter by a JSON field value using SQLite's `json_extract()`.
    ///
    /// ```sql
    /// WHERE json_extract("col", '$.key') = ?
    /// ```
    ///
    /// # Example
    /// ```rust,ignore
    /// Post::filter(&pool)
    ///     .json_extract_eq("metadata", "$.status", "published")
    ///     .all().await?
    /// ```
    fn json_extract_eq(self, col: &str, path: &str, value: &str) -> Self;

    /// Filter by a JSON field using LIKE.
    ///
    /// ```sql
    /// WHERE json_extract("col", '$.key') LIKE ?
    /// ```
    fn json_extract_like(self, col: &str, path: &str, pattern: &str) -> Self;

    /// Filter by a JSON field being NOT NULL (key exists).
    ///
    /// ```sql
    /// WHERE json_extract("col", '$.key') IS NOT NULL
    /// ```
    fn json_extract_exists(self, col: &str, path: &str) -> Self;

    /// Full-text search using SQLite FTS5.
    ///
    /// Requires a virtual FTS5 table created separately:
    /// ```sql
    /// CREATE VIRTUAL TABLE articles_fts USING fts5(content, content='articles', content_rowid='id');
    /// ```
    ///
    /// Then:
    /// ```sql
    /// WHERE rowid IN (SELECT rowid FROM fts_table WHERE fts_table MATCH ?)
    /// ```
    ///
    /// # Example
    /// ```rust,ignore
    /// Article::filter(&pool)
    ///     .fts5("articles_fts", "rust ORM")
    ///     .all().await?
    /// ```
    fn fts5(self, fts_table: &str, query: &str) -> Self;
}

impl<M> SqliteQueryExt<M> for QueryBuilder<M>
where
    M: Model + ModelValues + FromRow,
{
    fn json_extract_eq(self, col: &str, path: &str, value: &str) -> Self {
        self.raw_where(
            format!("json_extract(\"{}\", '{}') = ?", col, path),
            vec![SqlValue::Text(value.to_string())],
        )
    }

    fn json_extract_like(self, col: &str, path: &str, pattern: &str) -> Self {
        self.raw_where(
            format!("json_extract(\"{}\", '{}') LIKE ?", col, path),
            vec![SqlValue::Text(pattern.to_string())],
        )
    }

    fn json_extract_exists(self, col: &str, path: &str) -> Self {
        self.raw_where_no_bind(format!("json_extract(\"{}\", '{}') IS NOT NULL", col, path))
    }

    fn fts5(self, fts_table: &str, query: &str) -> Self {
        self.raw_where(
            format!(
                "rowid IN (SELECT rowid FROM \"{}\" WHERE \"{}\" MATCH ?)",
                fts_table, fts_table
            ),
            vec![SqlValue::Text(query.to_string())],
        )
    }
}
