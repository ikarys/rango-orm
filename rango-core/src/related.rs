//! Relation cache shared by the backends (`select_related` / `prefetch_related`).

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use crate::query::SqlValue;
use crate::row::{FromRow, RangoRow, RowError};

/// Type-erased relation storage, keyed by TypeId.
///
/// - Single FK relations (`related<R>`)      → key: `TypeId::of::<R>()`,       value: `Box<R>`
/// - Reverse/M2M relations (`prefetched<R>`) → key: `TypeId::of::<Vec<R>>()`,  value: `Box<Vec<R>>`
///   (different keys so both can coexist for the same R)
pub type RelationCache = HashMap<TypeId, Box<dyn Any + Send + Sync>>;

/// A model instance with an attached relation cache (level-1, in-process).
///
/// `Deref`s to the inner model so field access (`post.title`) works directly.
/// Relations are populated by `QueryBuilder::all()` when `.select_related()`
/// or `.prefetch_related()` were chained before execution.
///
/// # Example
/// ```rust,ignore
/// let posts = Post::filter(&pool)
///     .select_related(Post::AUTHOR_ID)
///     .prefetch_related(Comment::POST_ID)
///     .all()
///     .await?;
///
/// for post in &posts {
///     println!("{} by {}", post.title, post.related::<Author>().name);
///     for comment in post.prefetched::<Comment>() {
///         println!("  - {}", comment.body);
///     }
/// }
/// ```
pub struct WithRelated<M> {
    pub inner: M,
    cache: RelationCache,
}

impl<M> WithRelated<M> {
    /// Used by backends; relations are attached by `QueryBuilder::all()`.
    #[doc(hidden)]
    pub fn new(inner: M) -> Self {
        Self {
            inner,
            cache: HashMap::new(),
        }
    }

    /// Returns the FK-related instance loaded by `select_related`.
    ///
    /// # Panics
    /// Panics with a clear message if `.select_related(..)` was not called for `R`
    /// on the query builder.
    pub fn related<R: Any + Send + Sync + 'static>(&self) -> &R {
        self.cache
            .get(&TypeId::of::<R>())
            .and_then(|b| b.downcast_ref::<R>())
            .unwrap_or_else(|| {
                panic!(
                    "select_related<{ty}> was not loaded; \
                 add .select_related(..) for {ty} to the QueryBuilder chain",
                    ty = type_name::<R>()
                )
            })
    }

    /// Returns the prefetched slice loaded by `prefetch_related`.
    ///
    /// # Panics
    /// Panics with a clear message if `.prefetch_related(..)` was not called for `R`
    /// on the query builder.
    pub fn prefetched<R: Any + Send + Sync + 'static>(&self) -> &[R] {
        self.cache
            .get(&TypeId::of::<Vec<R>>())
            .and_then(|b| b.downcast_ref::<Vec<R>>())
            .map(Vec::as_slice)
            .unwrap_or_else(|| {
                panic!(
                    "prefetch_related<{ty}> was not loaded; \
                 add .prefetch_related(..) for {ty} to the QueryBuilder chain",
                    ty = type_name::<R>()
                )
            })
    }

    /// Low-level insert for type-erased callers in `QueryBuilder`.
    /// `type_id` must match the actual concrete type stored in `boxed`.
    #[doc(hidden)]
    pub fn insert_raw(&mut self, type_id: TypeId, boxed: Box<dyn Any + Send + Sync>) {
        self.cache.insert(type_id, boxed);
    }
}

impl<M> Deref for WithRelated<M> {
    type Target = M;
    fn deref(&self) -> &M {
        &self.inner
    }
}

impl<M> DerefMut for WithRelated<M> {
    fn deref_mut(&mut self) -> &mut M {
        &mut self.inner
    }
}

// ─── Type-erased helpers for the backends' relation specs ─────────────────────

/// Type-erased `R::from_row`.
#[doc(hidden)]
pub type FromRowFn = fn(&dyn RangoRow) -> Result<Box<dyn Any + Send + Sync>, RowError>;

/// Collects boxed `R`s into a boxed `Vec<R>`.
#[doc(hidden)]
pub type CollectVecFn = fn(Vec<Box<dyn Any + Send + Sync>>) -> Box<dyn Any + Send + Sync>;

#[doc(hidden)]
pub fn erased_from_row<R>(row: &dyn RangoRow) -> Result<Box<dyn Any + Send + Sync>, RowError>
where
    R: FromRow + Send + Sync + 'static,
{
    R::from_row(row).map(|v| Box::new(v) as Box<dyn Any + Send + Sync>)
}

#[doc(hidden)]
pub fn collect_vec_erased<R>(items: Vec<Box<dyn Any + Send + Sync>>) -> Box<dyn Any + Send + Sync>
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
    Box::new(typed)
}

/// Hashable key for a primary / foreign key value (UUID, integer or text).
#[doc(hidden)]
pub fn sql_value_key(v: &SqlValue) -> Option<String> {
    match v {
        SqlValue::Uuid(u) => Some(u.to_string()),
        SqlValue::BigInt(i) => Some(i.to_string()),
        SqlValue::Int(i) => Some(i.to_string()),
        SqlValue::SmallInt(i) => Some(i.to_string()),
        SqlValue::Text(s) => Some(s.clone()),
        _ => None,
    }
}

/// Reads `col` with the same type as `like` (a key of the other side) and returns
/// its [`sql_value_key`]. `None` when the column is NULL or does not decode.
#[doc(hidden)]
pub fn row_key(row: &dyn RangoRow, col: &str, like: &SqlValue) -> Option<String> {
    match like {
        SqlValue::Uuid(_) => row.get_uuid(col).ok().map(|v| v.to_string()),
        SqlValue::BigInt(_) => row.get_i64(col).ok().map(|v| v.to_string()),
        SqlValue::Int(_) => row.get_i32(col).ok().map(|v| v.to_string()),
        SqlValue::SmallInt(_) => row.get_i16(col).ok().map(|v| v.to_string()),
        SqlValue::Text(_) => row.get_string(col).ok(),
        _ => None,
    }
}
