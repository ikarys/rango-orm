use rango_core::related::{collect_vec_erased, erased_from_row};
use rango_core::{ForeignKeyRef, FromRow, Model, ModelValues};
use std::any::TypeId;
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
    ///     .select_related(Post::AUTHOR_ID) // `author_id: ForeignKey<Author>`, so R = Author
    ///     .all().await?;
    ///
    /// for post in &posts {
    ///     println!("{} by {}", post.title, post.related::<Author>().name);
    /// }
    /// ```
    pub fn select_related<R>(mut self, fk_col: impl ForeignKeyRef<M, R>) -> Self
    where
        R: Model + ModelValues + FromRow + Send + Sync + 'static,
    {
        let fk_col = fk_col.column_name();
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
            columns: R::schema().columns.into_iter().map(|c| c.name).collect(),
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
    ///     .prefetch_related(Comment::POST_ID) // `post_id: ForeignKey<Post>` on Comment
    ///     .all().await?;
    ///
    /// for post in &posts {
    ///     println!("{} has {} comments", post.title, post.prefetched::<Comment>().len());
    /// }
    /// ```
    pub fn prefetch_related<R>(mut self, related_fk_col: impl ForeignKeyRef<R, M>) -> Self
    where
        R: Model + ModelValues + FromRow + Send + Sync + 'static,
    {
        let related_fk_col = related_fk_col.column_name();
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
