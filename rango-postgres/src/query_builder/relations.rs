use rango_core::Result;
use rango_core::{FromRow, Model, ModelValues, RowError, SqlValue};
use std::any::{Any, TypeId};
// ─── Bind helpers ─────────────────────────────────────────────────────────────

use super::*;

impl<M> QueryBuilder<M>
where
    M: Model + ModelValues + FromRow,
{
    // ── Relation specs (chainable) ────────────────────────────────────────────
    /// Eagerly load the FK target `R` for every result row.
    ///
    /// The FK is a column on **M's** table: `m.fk_col = r.pk`.
    /// After `.all()`, access the related object via `wr.related::<R>()`.
    ///
    /// Multiple calls are allowed for different FK columns / related types.
    ///
    /// ```rust,ignore
    /// let posts: Vec<WithRelated<Post>> = Post::filter(&pool)
    ///     .select_related::<Author>("author_id")
    ///     .all().await?;
    ///
    /// for post in &posts {
    ///     println!("{} by {}", post.title, post.related::<Author>().name);
    /// }
    /// ```
    pub fn select_related<R>(mut self, fk_col: &str) -> Self
    where
        R: Model + ModelValues + FromRow + Send + Sync + 'static,
    {
        self.select_related_specs.push((
            fk_col.to_string(),
            R::table_name().to_string(),
            R::pk_column().to_string(),
        ));
        self.select_related.push(SelectRelatedSpec {
            fk_col: fk_col.to_string(),
            type_id: TypeId::of::<R>(),
            table_name: R::table_name(),
            pk_col: R::pk_column(),
            from_row: erased_from_row::<R>,
        });
        self
    }

    /// Prefetch all `R` rows whose FK column points back to M's PK.
    ///
    /// The FK is a column on **R's** table: `r.related_fk_col = m.pk`.
    /// After `.all()`, access the collection via `wr.prefetched::<R>()`.
    ///
    /// ```rust,ignore
    /// let posts: Vec<WithRelated<Post>> = Post::filter(&pool)
    ///     .prefetch_related::<Comment>("post_id")
    ///     .all().await?;
    ///
    /// for post in &posts {
    ///     println!("{} has {} comments", post.title, post.prefetched::<Comment>().len());
    /// }
    /// ```
    pub fn prefetch_related<R>(mut self, related_fk_col: &str) -> Self
    where
        R: Model + ModelValues + FromRow + Send + Sync + 'static,
    {
        self.prefetch_specs.push((
            R::table_name().to_string(),
            R::pk_column().to_string(),
            related_fk_col.to_string(),
        ));
        self.prefetch_related.push(PrefetchSpec {
            related_fk_col: related_fk_col.to_string(),
            vec_type_id: TypeId::of::<Vec<R>>(),
            table_name: R::table_name(),
            from_row: erased_from_row::<R>,
            collect_vec: collect_vec_erased::<R>,
        });
        self
    }
}

// ─── Type-erased helpers (monomorphized at call site) ─────────────────────────
pub(super) fn erased_from_row<R>(
    row: &dyn rango_core::RangoRow,
) -> Result<Box<dyn Any + Send + Sync>, RowError>
where
    R: FromRow + Send + Sync + 'static,
{
    R::from_row(row).map(|v| Box::new(v) as Box<dyn Any + Send + Sync>)
}

pub(super) fn collect_vec_erased<R>(
    items: Vec<Box<dyn Any + Send + Sync>>,
) -> Box<dyn Any + Send + Sync>
where
    R: Any + Send + Sync + 'static,
{
    let typed: Vec<R> = items
        .into_iter()
        .map(|b| {
            *b.downcast::<R>()
                .expect("prefetch_related type mismatch — this is a bug")
        })
        .collect();
    Box::new(typed) as Box<dyn Any + Send + Sync>
}

// ─── Row key extraction helpers ───────────────────────────────────────────────
/// Convert a `SqlValue` to a string key for use in HashMap lookups.
pub(super) fn sql_value_key(v: &SqlValue) -> Option<String> {
    match v {
        SqlValue::Uuid(u) => Some(u.to_string()),
        SqlValue::BigInt(i) => Some(i.to_string()),
        SqlValue::Int(i) => Some(i.to_string()),
        SqlValue::SmallInt(i) => Some(i.to_string()),
        SqlValue::Text(s) => Some(s.clone()),
        _ => None,
    }
}

/// Extract a string key for `col` from a raw `PgRow`.
/// Tries UUID, i64, i32, i16, String in order.
pub(super) fn extract_key_from_row(row: &sqlx::postgres::PgRow, col: &str) -> Option<String> {
    use sqlx::Row;
    if let Ok(v) = row.try_get::<uuid::Uuid, _>(col) {
        return Some(v.to_string());
    }
    if let Ok(v) = row.try_get::<i64, _>(col) {
        return Some(v.to_string());
    }
    if let Ok(v) = row.try_get::<i32, _>(col) {
        return Some(v.to_string());
    }
    if let Ok(v) = row.try_get::<i16, _>(col) {
        return Some(v.to_string());
    }
    if let Ok(v) = row.try_get::<String, _>(col) {
        return Some(v);
    }
    None
}
