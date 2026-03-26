use std::any::{type_name, Any, TypeId};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

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
///     .select_related::<Author>("author_id")
///     .prefetch_related::<Comment>("post_id")
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
    pub(crate) fn new(inner: M) -> Self {
        Self { inner, cache: HashMap::new() }
    }

    /// Returns the FK-related instance loaded by `select_related`.
    ///
    /// # Panics
    /// Panics with a clear message if `.select_related::<R>()` was not called
    /// on the query builder.
    pub fn related<R: Any + Send + Sync + 'static>(&self) -> &R {
        self.cache
            .get(&TypeId::of::<R>())
            .and_then(|b| b.downcast_ref::<R>())
            .unwrap_or_else(|| panic!(
                "select_related<{ty}> was not loaded; \
                 add .select_related::<{ty}>(\"col\") to the QueryBuilder chain",
                ty = type_name::<R>()
            ))
    }

    /// Returns the prefetched slice loaded by `prefetch_related`.
    ///
    /// # Panics
    /// Panics with a clear message if `.prefetch_related::<R>()` was not called
    /// on the query builder.
    pub fn prefetched<R: Any + Send + Sync + 'static>(&self) -> &[R] {
        self.cache
            .get(&TypeId::of::<Vec<R>>())
            .and_then(|b| b.downcast_ref::<Vec<R>>())
            .map(Vec::as_slice)
            .unwrap_or_else(|| panic!(
                "prefetch_related<{ty}> was not loaded; \
                 add .prefetch_related::<{ty}>(\"col\") to the QueryBuilder chain",
                ty = type_name::<R>()
            ))
    }

    /// Attach a prefetched one-to-many collection (called by `QueryBuilder::all()`).
    pub(crate) fn set_prefetched<R: Any + Send + Sync + 'static>(&mut self, items: Vec<R>) {
        self.cache.insert(TypeId::of::<Vec<R>>(), Box::new(items));
    }

    /// Attach a single FK-related object (called by `QueryBuilder::all()`).
    pub(crate) fn set_related<R: Any + Send + Sync + 'static>(&mut self, item: R) {
        self.cache.insert(TypeId::of::<R>(), Box::new(item));
    }

    /// Low-level insert for type-erased callers in `QueryBuilder`.
    /// `type_id` must match the actual concrete type stored in `boxed`.
    pub(crate) fn insert_raw(&mut self, type_id: TypeId, boxed: Box<dyn Any + Send + Sync>) {
        self.cache.insert(type_id, boxed);
    }
}

impl<M> Deref for WithRelated<M> {
    type Target = M;
    fn deref(&self) -> &M { &self.inner }
}

impl<M> DerefMut for WithRelated<M> {
    fn deref_mut(&mut self) -> &mut M { &mut self.inner }
}
