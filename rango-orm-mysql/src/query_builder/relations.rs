use rango_orm_core::related::{collect_vec_erased, erased_from_row, m2m_pivot};
use rango_orm_core::{
    quote_ident_mysql as quote_ident, ForeignKeyRef, FromRow, Model, ModelValues, PrefetchKind,
    PrefetchRef,
};
use std::any::TypeId;

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

    /// Prefetch all `R` rows related to M, either by a reverse FK or a M2M pivot.
    ///
    /// For a reverse FK, the FK is a column on **R's** table: `r.related_fk_col = m.pk`.
    /// For a M2M field, the pivot table links M and R.
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
    ///
    /// ```rust,ignore
    /// let articles = Article::filter(&pool)
    ///     .prefetch_related(Article::TAGS) // `tags: ManyToMany<Tag>` on Article
    ///     .all().await?;
    /// for a in &articles { println!("{} tags", a.prefetched::<Tag>().len()); }
    /// ```
    pub fn prefetch_related<R>(mut self, target: impl PrefetchRef<M, R>) -> Self
    where
        R: Model + ModelValues + FromRow + Send + Sync + 'static,
    {
        let (select, key_col) = match target.prefetch_kind() {
            PrefetchKind::ReverseFk(col) => (
                format!(
                    "SELECT * FROM {} WHERE {}",
                    quote_ident(R::table_name()),
                    quote_ident(col),
                ),
                col.to_string(),
            ),
            PrefetchKind::ManyToMany => {
                let (pivot, from_col, to_col) = m2m_pivot(M::table_name(), R::table_name());
                let (p, t) = (quote_ident("__p"), quote_ident("__t"));
                let select = format!(
                    "SELECT {p}.{from} AS {key}, {t}.* FROM {to} AS {t} \
                     INNER JOIN {pivot} AS {p} ON {t}.{pk} = {p}.{to_col} WHERE {p}.{from}",
                    from = quote_ident(&from_col),
                    key = quote_ident("__m2m_key"),
                    to = quote_ident(R::table_name()),
                    pivot = quote_ident(&pivot),
                    pk = quote_ident(R::pk_column()),
                    to_col = quote_ident(&to_col),
                );
                (select, "__m2m_key".to_string())
            }
        };
        self.prefetch_related.push(PrefetchSpec {
            select,
            key_col,
            vec_type_id: TypeId::of::<Vec<R>>(),
            from_row: erased_from_row::<R>,
            collect_vec: collect_vec_erased::<R>,
        });
        self
    }
}
