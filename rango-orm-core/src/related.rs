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
/// M2M pivot naming shared by `M2M` ops, `prefetch_related` and `rango makemigrations`:
/// `(pivot_table, from_col, to_col)` = `("{from}_{to}", "{from}_id", "{to}_id")`.
/// A self-referencing relation (`from == to`) would get two columns with the same name,
/// so its columns are `from_{x}_id` and `to_{x}_id`.
pub fn m2m_pivot(from_table: &str, to_table: &str) -> (String, String, String) {
    let (from_col, to_col) = if from_table == to_table {
        (format!("from_{from_table}_id"), format!("to_{to_table}_id"))
    } else {
        (format!("{from_table}_id"), format!("{to_table}_id"))
    };
    (format!("{from_table}_{to_table}"), from_col, to_col)
}

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

#[cfg(test)]
mod pivot_tests {
    use super::m2m_pivot;

    #[test]
    fn pivot_columns_are_distinct_for_self_reference() {
        let s = |t: &str| t.to_string();
        assert_eq!(m2m_pivot("a", "b"), (s("a_b"), s("a_id"), s("b_id")));
        assert_eq!(
            m2m_pivot("u", "u"),
            (s("u_u"), s("from_u_id"), s("to_u_id"))
        );
    }
}

/// The SQL of the many-to-many pivot operations, for one dialect.
///
/// Every statement binds the from id first, then the to id.
#[doc(hidden)]
pub struct M2mSql {
    dialect: crate::Dialect,
    pivot: String,
    from_col: String,
    to_col: String,
    from_table: String,
    to_table: String,
}

impl M2mSql {
    pub fn new(dialect: crate::Dialect, from_table: &str, to_table: &str) -> Self {
        let (pivot, from_col, to_col) = m2m_pivot(from_table, to_table);
        Self {
            dialect,
            pivot,
            from_col,
            to_col,
            from_table: from_table.to_owned(),
            to_table: to_table.to_owned(),
        }
    }

    /// Unquoted pivot table name (for tracing).
    pub fn pivot_table(&self) -> &str {
        &self.pivot
    }

    fn q(&self, ident: &str) -> String {
        self.dialect.quote_ident(ident)
    }

    fn ph(&self, n: usize) -> String {
        match self.dialect {
            crate::Dialect::Postgres => format!("${n}"),
            _ => "?".to_owned(),
        }
    }

    /// Idempotent insert of a `(from, to)` pair.
    pub fn insert(&self) -> String {
        let (pivot, from, to) = (
            self.q(&self.pivot),
            self.q(&self.from_col),
            self.q(&self.to_col),
        );
        let (p1, p2) = (self.ph(1), self.ph(2));
        match self.dialect {
            // ON DUPLICATE KEY UPDATE, not INSERT IGNORE, to avoid silencing FK and other errors.
            crate::Dialect::Mysql => format!(
                "INSERT INTO {pivot} ({from}, {to}) VALUES ({p1}, {p2}) \
                 ON DUPLICATE KEY UPDATE {from} = {from}"
            ),
            _ => format!(
                "INSERT INTO {pivot} ({from}, {to}) VALUES ({p1}, {p2}) ON CONFLICT DO NOTHING"
            ),
        }
    }

    /// Delete every pair of one `from` id.
    pub fn clear(&self) -> String {
        format!(
            "DELETE FROM {} WHERE {} = {}",
            self.q(&self.pivot),
            self.q(&self.from_col),
            self.ph(1)
        )
    }

    /// Delete one `(from, to)` pair.
    pub fn remove(&self) -> String {
        format!(
            "DELETE FROM {} WHERE {} = {} AND {} = {}",
            self.q(&self.pivot),
            self.q(&self.from_col),
            self.ph(1),
            self.q(&self.to_col),
            self.ph(2)
        )
    }

    /// Does the `(from, to)` pair exist.
    pub fn exists(&self) -> String {
        format!(
            "SELECT 1 FROM {} WHERE {} = {} AND {} = {} LIMIT 1",
            self.q(&self.pivot),
            self.q(&self.from_col),
            self.ph(1),
            self.q(&self.to_col),
            self.ph(2)
        )
    }

    /// The `to` rows of one `from` id (bind: from id).
    pub fn all(&self, to_pk: &str) -> String {
        format!(
            r#"SELECT t.* FROM {to_table} t
               INNER JOIN {pivot} p ON t.{to_pk} = p.{to_col}
               WHERE p.{from_col} = {p1}"#,
            to_table = self.q(&self.to_table),
            pivot = self.q(&self.pivot),
            to_pk = self.q(to_pk),
            to_col = self.q(&self.to_col),
            from_col = self.q(&self.from_col),
            p1 = self.ph(1),
        )
    }

    /// The `from` rows of one `to` id (bind: to id).
    pub fn reverse(&self, from_pk: &str) -> String {
        format!(
            r#"SELECT f.* FROM {from_table} f
               INNER JOIN {pivot} p ON f.{from_pk} = p.{from_col}
               WHERE p.{to_col} = {p1}"#,
            from_table = self.q(&self.from_table),
            pivot = self.q(&self.pivot),
            from_pk = self.q(from_pk),
            from_col = self.q(&self.from_col),
            to_col = self.q(&self.to_col),
            p1 = self.ph(1),
        )
    }
}

#[cfg(test)]
mod m2m_sql_tests {
    use super::M2mSql;
    use crate::Dialect;

    #[test]
    fn per_dialect_sql() {
        let pg = M2mSql::new(Dialect::Postgres, "post", "tag");
        assert_eq!(
            pg.insert(),
            "INSERT INTO \"post_tag\" (\"post_id\", \"tag_id\") VALUES ($1, $2) ON CONFLICT DO NOTHING"
        );
        assert_eq!(
            pg.remove(),
            "DELETE FROM \"post_tag\" WHERE \"post_id\" = $1 AND \"tag_id\" = $2"
        );
        let my = M2mSql::new(Dialect::Mysql, "post", "tag");
        assert_eq!(
            my.insert(),
            "INSERT INTO `post_tag` (`post_id`, `tag_id`) VALUES (?, ?) \
             ON DUPLICATE KEY UPDATE `post_id` = `post_id`"
        );
        let lite = M2mSql::new(Dialect::Sqlite, "post", "tag");
        assert_eq!(
            lite.clear(),
            "DELETE FROM \"post_tag\" WHERE \"post_id\" = ?"
        );
    }
}
