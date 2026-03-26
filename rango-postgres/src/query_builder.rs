use std::any::{Any, TypeId};
use std::collections::HashMap;

use rango_core::{FromRow, Model, ModelValues, RowError, SqlValue};
use sqlx::{PgPool, Postgres};
use anyhow::Result;

use crate::ops::DEFAULT_QUERY_LIMIT;
use crate::related::WithRelated;
use crate::row::{PgRangoRow, PgRangoRow2};

// ─── Filter types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Op {
    Eq, Ne, Gt, Gte, Lt, Lte, Like, ILike, In, IsNull, IsNotNull,
}

#[derive(Debug, Clone)]
struct Condition {
    column: String,
    op: Op,
    value: Option<SqlValue>,
    values: Option<Vec<SqlValue>>,  // for IN
}

#[derive(Debug, Clone)]
enum Connector { And, Or, AndNot, OrNot, Xor }

#[derive(Debug, Clone)]
enum ConditionNode {
    Single(Condition),
    Group(Vec<ConditionGroup>),
}

#[derive(Debug, Clone)]
struct ConditionGroup {
    node: ConditionNode,
    connector: Connector,
}

// ─── Relation specs ───────────────────────────────────────────────────────────

/// Spec for a `.select_related::<R>(fk_col)` call.
/// The FK is on the main model's table: `m.fk_col = r.pk`.
struct SelectRelatedSpec {
    /// Column on M's table containing the FK to R.
    fk_col: String,
    /// `TypeId::of::<R>()` — used as the `WithRelated` cache key.
    type_id: TypeId,
    /// R's table name.
    table_name: &'static str,
    /// R's primary key column name.
    pk_col: &'static str,
    /// Type-erased `R::from_row`.
    from_row: fn(&dyn rango_core::RangoRow) -> Result<Box<dyn Any + Send + Sync>, RowError>,
}

/// Spec for a `.prefetch_related::<R>(related_fk_col)` call.
/// The FK is on R's table: `r.related_fk_col = m.pk`.
struct PrefetchSpec {
    /// Column on R's table pointing back to M.
    related_fk_col: String,
    /// `TypeId::of::<Vec<R>>()` — used as the `WithRelated` cache key.
    vec_type_id: TypeId,
    /// R's table name.
    table_name: &'static str,
    /// Type-erased `R::from_row`.
    from_row: fn(&dyn rango_core::RangoRow) -> Result<Box<dyn Any + Send + Sync>, RowError>,
    /// Collects `Vec<Box<dyn Any>>` into a `Box<dyn Any>` containing `Vec<R>`.
    collect_vec: fn(Vec<Box<dyn Any + Send + Sync>>) -> Box<dyn Any + Send + Sync>,
}

// ─── QueryBuilder ─────────────────────────────────────────────────────────────

/// Chainable, lazy query builder. SQL is built and executed only at `.all()` / `.one()`.
pub struct QueryBuilder<M> {
    pool: PgPool,
    // TODO: cache backend trait (level 2 — Redis, Memcached, custom)
    conditions: Vec<ConditionGroup>,
    order_by: Vec<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    /// True when the caller has explicitly set a limit via `.limit(n)` or `.unlimited()`.
    /// When false, `all()` applies [`DEFAULT_QUERY_LIMIT`] automatically.
    pub explicit_limit: bool,
    next_connector: Connector,
    /// Introspection: `(fk_col, related_table, related_pk)` for each `.select_related()` call.
    pub select_related_specs: Vec<(String, String, String)>,
    /// Introspection: `(related_table, related_pk, fk_col)` for each `.prefetch_related()` call.
    pub prefetch_specs: Vec<(String, String, String)>,
    select_related: Vec<SelectRelatedSpec>,
    prefetch_related: Vec<PrefetchSpec>,
    _phantom: std::marker::PhantomData<M>,
}

impl<M> QueryBuilder<M>
where
    M: Model + ModelValues + FromRow,
{
    pub fn new(pool: &PgPool) -> Self {
        Self {
            pool: pool.clone(),
            conditions: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
            explicit_limit: false,
            next_connector: Connector::And,
            select_related_specs: Vec::new(),
            prefetch_specs: Vec::new(),
            select_related: Vec::new(),
            prefetch_related: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    // ── Filter methods ────────────────────────────────────────────────────────

    /// WHERE col = value
    pub fn eq(self, col: &str, val: impl Into<SqlValue>) -> Self {
        self.add(col, Op::Eq, Some(val.into()), None)
    }

    /// WHERE col != value
    pub fn ne(self, col: &str, val: impl Into<SqlValue>) -> Self {
        self.add(col, Op::Ne, Some(val.into()), None)
    }

    /// WHERE col > value
    pub fn gt(self, col: &str, val: impl Into<SqlValue>) -> Self {
        self.add(col, Op::Gt, Some(val.into()), None)
    }

    /// WHERE col >= value
    pub fn gte(self, col: &str, val: impl Into<SqlValue>) -> Self {
        self.add(col, Op::Gte, Some(val.into()), None)
    }

    /// WHERE col < value
    pub fn lt(self, col: &str, val: impl Into<SqlValue>) -> Self {
        self.add(col, Op::Lt, Some(val.into()), None)
    }

    /// WHERE col <= value
    pub fn lte(self, col: &str, val: impl Into<SqlValue>) -> Self {
        self.add(col, Op::Lte, Some(val.into()), None)
    }

    /// WHERE col LIKE pattern
    pub fn like(self, col: &str, pattern: impl Into<String>) -> Self {
        self.add(col, Op::Like, Some(SqlValue::Text(pattern.into())), None)
    }

    /// WHERE col ILIKE pattern (case-insensitive, PostgreSQL)
    pub fn ilike(self, col: &str, pattern: impl Into<String>) -> Self {
        self.add(col, Op::ILike, Some(SqlValue::Text(pattern.into())), None)
    }

    /// WHERE col IN (values)
    pub fn in_values(self, col: &str, vals: Vec<SqlValue>) -> Self {
        self.add(col, Op::In, None, Some(vals))
    }

    /// WHERE col IS NULL
    pub fn is_null(self, col: &str) -> Self {
        self.add(col, Op::IsNull, None, None)
    }

    /// WHERE col IS NOT NULL
    pub fn is_not_null(self, col: &str) -> Self {
        self.add(col, Op::IsNotNull, None, None)
    }

    // ── Connector methods ─────────────────────────────────────────────────────

    /// Next condition uses OR
    pub fn or(mut self) -> Self {
        self.next_connector = Connector::Or;
        self
    }

    /// Next condition uses AND NOT
    pub fn not(mut self) -> Self {
        self.next_connector = Connector::AndNot;
        self
    }

    /// Next condition uses OR NOT
    pub fn or_not(mut self) -> Self {
        self.next_connector = Connector::OrNot;
        self
    }

    /// Next condition uses XOR (PostgreSQL only)
    pub fn xor(mut self) -> Self {
        self.next_connector = Connector::Xor;
        self
    }

    /// Group conditions in parentheses.
    ///
    /// ```rust
    /// User::filter(&pool)
    ///     .eq("active", true)
    ///     .group(|q| q.eq("role", "admin").or().eq("role", "moderator"))
    ///     .all().await?
    /// // → WHERE "active" = $1 AND ("role" = $2 OR "role" = $3)
    /// ```
    pub fn group<F>(mut self, f: F) -> Self
    where
        F: FnOnce(QueryBuilder<M>) -> QueryBuilder<M>,
    {
        let inner = QueryBuilder::<M>::new(&self.pool);
        let inner = f(inner);
        let connector = std::mem::replace(&mut self.next_connector, Connector::And);
        self.conditions.push(ConditionGroup {
            node: ConditionNode::Group(inner.conditions),
            connector,
        });
        self
    }

    // ── Ordering / pagination ─────────────────────────────────────────────────

    /// ORDER BY col ASC, or `-col` for DESC.
    pub fn order_by(mut self, col: &str) -> Self {
        let order = if col.starts_with('-') {
            format!("\"{}\" DESC", &col[1..])
        } else {
            format!("\"{}\" ASC", col)
        };
        self.order_by.push(order);
        self
    }

    /// Apply a LIMIT and mark it as explicit (suppresses the automatic 1000-row cap).
    pub fn limit(mut self, n: i64) -> Self {
        self.limit = Some(n);
        self.explicit_limit = true;
        self
    }

    pub fn offset(mut self, n: i64) -> Self {
        self.offset = Some(n);
        self
    }

    /// Disable the default row cap — returns all rows with no LIMIT clause.
    ///
    /// Use with care on large tables. The default cap is [`DEFAULT_QUERY_LIMIT`].
    pub fn unlimited(mut self) -> Self {
        self.explicit_limit = true;
        self
    }

    // ── Relation specs (chainable) ────────────────────────────────────────────

    /// Eagerly load the FK target `R` for every result row.
    ///
    /// The FK is a column on **M's** table: `m.fk_col = r.pk`.
    /// After `.all()`, access the related object via `wr.related::<R>()`.
    ///
    /// Multiple calls are allowed for different FK columns / related types.
    ///
    /// ```rust
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
    /// ```rust
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

    // ── Terminal methods ──────────────────────────────────────────────────────

    /// Fetch all matching rows, populating relation caches when specs are present.
    ///
    /// Returns `Vec<WithRelated<M>>` — each item derefs to `M`, and related
    /// objects are accessible via `.related::<R>()` / `.prefetched::<R>()`.
    pub async fn all(self) -> Result<Vec<WithRelated<M>>> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();

        let mut sql = format!("SELECT * FROM \"{}\"", table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        if !self.order_by.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", self.order_by.join(", ")));
        }
        // Apply the default row cap when neither .limit() nor .unlimited() was called.
        if !self.explicit_limit {
            sql.push_str(&format!(" LIMIT {}", DEFAULT_QUERY_LIMIT));
        } else if let Some(l) = self.limit {
            sql.push_str(&format!(" LIMIT {}", l));
        }
        // explicit_limit=true, limit=None → .unlimited() was called; no LIMIT clause added
        if let Some(o) = self.offset { sql.push_str(&format!(" OFFSET {}", o)); }

        let rows = bind_and_fetch_all(&self.pool, &sql, binds).await?;
        let mut results: Vec<WithRelated<M>> = rows
            .into_iter()
            .map(|r| {
                M::from_row(&PgRangoRow(r))
                    .map(WithRelated::new)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .collect::<Result<Vec<_>>>()?;

        if results.is_empty() {
            return Ok(results);
        }

        // ── select_related: FK on M, bulk-fetch R by PK ───────────────────────
        for spec in &self.select_related {
            // Pair each result index with its FK value.
            let indexed_fks: Vec<(usize, SqlValue)> = results
                .iter()
                .enumerate()
                .filter_map(|(i, wr)| {
                    wr.inner
                        .field_values()
                        .into_iter()
                        .find(|(c, _)| *c == spec.fk_col.as_str())
                        .map(|(_, v)| (i, v))
                })
                .collect();

            if indexed_fks.is_empty() { continue; }

            // Deduplicate FK values while preserving order.
            let mut seen: Vec<String> = Vec::new();
            let mut unique_vals: Vec<SqlValue> = Vec::new();
            for (_, v) in &indexed_fks {
                if let Some(k) = sql_value_key(v) {
                    if !seen.contains(&k) {
                        seen.push(k);
                        unique_vals.push(v.clone());
                    }
                }
            }
            if unique_vals.is_empty() { continue; }

            let placeholders: Vec<String> =
                (1..=unique_vals.len()).map(|i| format!("${}", i)).collect();
            let related_sql = format!(
                "SELECT * FROM \"{}\" WHERE \"{}\" IN ({})",
                spec.table_name,
                spec.pk_col,
                placeholders.join(", "),
            );

            let related_rows = bind_and_fetch_all(&self.pool, &related_sql, unique_vals).await?;

            // Map pk key → PgRow (re-used for each M that shares the same FK).
            let mut row_by_pk: HashMap<String, sqlx::postgres::PgRow> = HashMap::new();
            for row in related_rows {
                if let Some(k) = extract_key_from_row(&row, spec.pk_col) {
                    row_by_pk.insert(k, row);
                }
            }

            for (i, fk_val) in &indexed_fks {
                if let Some(fk_key) = sql_value_key(fk_val) {
                    if let Some(row) = row_by_pk.get(&fk_key) {
                        let boxed = (spec.from_row)(&PgRangoRow2::new(row))
                            .map_err(|e| anyhow::anyhow!("select_related({}): {}", spec.fk_col, e))?;
                        results[*i].insert_raw(spec.type_id, boxed);
                    }
                }
            }
        }

        // ── prefetch_related: FK on R, bulk-fetch and group by M PK ──────────
        for spec in &self.prefetch_related {
            let indexed_pks: Vec<(usize, SqlValue)> = results
                .iter()
                .enumerate()
                .map(|(i, wr)| (i, wr.inner.pk_value()))
                .collect();

            let mut seen: Vec<String> = Vec::new();
            let mut unique_pks: Vec<SqlValue> = Vec::new();
            for (_, v) in &indexed_pks {
                if let Some(k) = sql_value_key(v) {
                    if !seen.contains(&k) {
                        seen.push(k);
                        unique_pks.push(v.clone());
                    }
                }
            }
            if unique_pks.is_empty() { continue; }

            let placeholders: Vec<String> =
                (1..=unique_pks.len()).map(|i| format!("${}", i)).collect();
            let prefetch_sql = format!(
                "SELECT * FROM \"{}\" WHERE \"{}\" IN ({})",
                spec.table_name,
                spec.related_fk_col,
                placeholders.join(", "),
            );

            let prefetch_rows = bind_and_fetch_all(&self.pool, &prefetch_sql, unique_pks).await?;

            // Group boxed R items by the FK value they carry.
            let mut grouped: HashMap<String, Vec<Box<dyn Any + Send + Sync>>> = HashMap::new();
            for row in &prefetch_rows {
                if let Some(fk_key) = extract_key_from_row(row, &spec.related_fk_col) {
                    let boxed = (spec.from_row)(&PgRangoRow2::new(row))
                        .map_err(|e| anyhow::anyhow!("prefetch_related({}): {}", spec.related_fk_col, e))?;
                    grouped.entry(fk_key).or_default().push(boxed);
                }
            }

            // Attach to each result — always set the cache (empty Vec for no matches).
            for (i, pk_val) in &indexed_pks {
                let items = sql_value_key(pk_val)
                    .and_then(|k| grouped.remove(&k))
                    .unwrap_or_default();
                let boxed_vec = (spec.collect_vec)(items);
                results[*i].insert_raw(spec.vec_type_id, boxed_vec);
            }
        }

        Ok(results)
    }

    /// Fetch the first matching row (no relation population).
    pub async fn one(self) -> Result<Option<WithRelated<M>>> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();

        let mut sql = format!("SELECT * FROM \"{}\"", table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        sql.push_str(" LIMIT 1");

        let rows = bind_and_fetch_all(&self.pool, &sql, binds).await?;
        match rows.into_iter().next() {
            Some(r) => Ok(Some(
                M::from_row(&PgRangoRow(r))
                    .map(WithRelated::new)
                    .map_err(|e| anyhow::anyhow!("{}", e))?,
            )),
            None => Ok(None),
        }
    }

    /// Count matching rows.
    pub async fn count(self) -> Result<i64> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();

        let mut sql = format!("SELECT COUNT(*) FROM \"{}\"", table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }

        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<i64, _>(0).unwrap_or(0))
    }

    /// Check if any matching row exists.
    pub async fn exists(self) -> Result<bool> {
        Ok(self.count().await? > 0)
    }

    /// SELECT SUM(col) FROM ... WHERE ... — returns None if the table is empty or all values are NULL.
    pub async fn sum(self, col: &str) -> Result<Option<f64>> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT SUM(\"{}\") FROM \"{}\"", col, table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<Option<f64>, _>(0).unwrap_or(None))
    }

    /// SELECT AVG(col) FROM ... WHERE ... — returns None if the table is empty or all values are NULL.
    pub async fn avg(self, col: &str) -> Result<Option<f64>> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT AVG(\"{}\") FROM \"{}\"", col, table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<Option<f64>, _>(0).unwrap_or(None))
    }

    /// SELECT MIN(col) FROM ... WHERE ...
    ///
    /// Generic over the return type — works for numbers, strings, dates, etc.
    /// Returns `None` if the table is empty or all values are NULL.
    ///
    /// # Example
    /// ```rust
    /// let oldest: Option<NaiveDate> = Event::filter(&pool).min::<NaiveDate>("date").await?;
    /// let lowest: Option<f64>       = Order::filter(&pool).min::<f64>("price").await?;
    /// ```
    pub async fn min<T>(self, col: &str) -> Result<Option<T>>
    where
        T: for<'r> sqlx::Decode<'r, Postgres> + sqlx::Type<Postgres>,
    {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT MIN(\"{}\") FROM \"{}\"", col, table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<Option<T>, _>(0).unwrap_or(None))
    }

    /// SELECT MAX(col) FROM ... WHERE ...
    ///
    /// Generic over the return type — works for numbers, strings, dates, etc.
    /// Returns `None` if the table is empty or all values are NULL.
    ///
    /// # Example
    /// ```rust
    /// let latest: Option<DateTime<Utc>> = Post::filter(&pool).max::<DateTime<Utc>>("created_at").await?;
    /// let highest: Option<f64>          = Order::filter(&pool).max::<f64>("price").await?;
    /// ```
    pub async fn max<T>(self, col: &str) -> Result<Option<T>>
    where
        T: for<'r> sqlx::Decode<'r, Postgres> + sqlx::Type<Postgres>,
    {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT MAX(\"{}\") FROM \"{}\"", col, table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<Option<T>, _>(0).unwrap_or(None))
    }

    /// Returns the SQL that would be executed by `.all()`, for debugging.
    pub fn explain(&self) -> String {
        let table = M::table_name();
        let (where_clause, _) = self.build_where();
        let mut sql = format!("SELECT * FROM \"{}\"", table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        if !self.order_by.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", self.order_by.join(", ")));
        }
        if let Some(l) = self.limit { sql.push_str(&format!(" LIMIT {}", l)); }
        else if !self.explicit_limit { sql.push_str(&format!(" LIMIT {}", DEFAULT_QUERY_LIMIT)); }
        if let Some(o) = self.offset { sql.push_str(&format!(" OFFSET {}", o)); }
        sql
    }

    /// Execute a closure within a transaction, using this builder's pool.
    pub async fn transaction<F, Fut, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut crate::transaction::RangoTransaction<'_>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        crate::transaction::atomic(&self.pool, f).await
    }

    // ── Internals ─────────────────────────────────────────────────────────────

    fn add(mut self, col: &str, op: Op, value: Option<SqlValue>, values: Option<Vec<SqlValue>>) -> Self {
        let connector = std::mem::replace(&mut self.next_connector, Connector::And);
        self.conditions.push(ConditionGroup {
            node: ConditionNode::Single(Condition { column: col.to_string(), op, value, values }),
            connector,
        });
        self
    }

    fn build_where(&self) -> (String, Vec<SqlValue>) {
        if self.conditions.is_empty() {
            return (String::new(), Vec::new());
        }
        let mut binds = Vec::new();
        let mut idx = 1usize;
        let expr = build_conditions(&self.conditions, &mut binds, &mut idx);
        (format!("WHERE {}", expr), binds)
    }
}

// ─── Type-erased helpers (monomorphized at call site) ─────────────────────────

fn erased_from_row<R>(row: &dyn rango_core::RangoRow) -> Result<Box<dyn Any + Send + Sync>, RowError>
where
    R: FromRow + Send + Sync + 'static,
{
    R::from_row(row).map(|v| Box::new(v) as Box<dyn Any + Send + Sync>)
}

fn collect_vec_erased<R>(items: Vec<Box<dyn Any + Send + Sync>>) -> Box<dyn Any + Send + Sync>
where
    R: Any + Send + Sync + 'static,
{
    let typed: Vec<R> = items
        .into_iter()
        .map(|b| *b.downcast::<R>().expect("prefetch_related type mismatch — this is a bug"))
        .collect();
    Box::new(typed) as Box<dyn Any + Send + Sync>
}

// ─── SQL builder helpers ──────────────────────────────────────────────────────

fn build_conditions(groups: &[ConditionGroup], binds: &mut Vec<SqlValue>, idx: &mut usize) -> String {
    let mut parts = Vec::new();
    for (i, cg) in groups.iter().enumerate() {
        let expr = match &cg.node {
            ConditionNode::Single(c) => build_condition(c, binds, idx),
            ConditionNode::Group(inner) => format!("({})", build_conditions(inner, binds, idx)),
        };
        if i == 0 {
            let expr = match &cg.connector {
                Connector::AndNot | Connector::OrNot => format!("NOT {}", expr),
                _ => expr,
            };
            parts.push(expr);
        } else {
            let conn = match &cg.connector {
                Connector::And    => format!("AND {}", expr),
                Connector::Or     => format!("OR {}", expr),
                Connector::AndNot => format!("AND NOT {}", expr),
                Connector::OrNot  => format!("OR NOT {}", expr),
                Connector::Xor    => format!("XOR {}", expr),
            };
            parts.push(conn);
        }
    }
    parts.join(" ")
}

fn build_condition(c: &Condition, binds: &mut Vec<SqlValue>, idx: &mut usize) -> String {
    let col = format!("\"{}\"", c.column);
    match &c.op {
        Op::IsNull    => format!("{} IS NULL", col),
        Op::IsNotNull => format!("{} IS NOT NULL", col),
        Op::In => {
            let vals = c.values.as_ref().unwrap();
            let placeholders: Vec<String> = vals.iter().map(|_| {
                let p = format!("${}", idx); *idx += 1; p
            }).collect();
            binds.extend(vals.clone());
            format!("{} IN ({})", col, placeholders.join(", "))
        }
        op => {
            let op_str = match op {
                Op::Eq    => "=",
                Op::Ne    => "!=",
                Op::Gt    => ">",
                Op::Gte   => ">=",
                Op::Lt    => "<",
                Op::Lte   => "<=",
                Op::Like  => "LIKE",
                Op::ILike => "ILIKE",
                _ => unreachable!(),
            };
            let p = format!("${}", idx); *idx += 1;
            binds.push(c.value.clone().unwrap());
            format!("{} {} {}", col, op_str, p)
        }
    }
}

// ─── Row key extraction helpers ───────────────────────────────────────────────

/// Convert a `SqlValue` to a string key for use in HashMap lookups.
fn sql_value_key(v: &SqlValue) -> Option<String> {
    match v {
        SqlValue::Uuid(u)      => Some(u.to_string()),
        SqlValue::BigInt(i)    => Some(i.to_string()),
        SqlValue::Int(i)       => Some(i.to_string()),
        SqlValue::SmallInt(i)  => Some(i.to_string()),
        SqlValue::Text(s)      => Some(s.clone()),
        _                      => None,
    }
}

/// Extract a string key for `col` from a raw `PgRow`.
/// Tries UUID, i64, i32, i16, String in order.
fn extract_key_from_row(row: &sqlx::postgres::PgRow, col: &str) -> Option<String> {
    use sqlx::Row;
    if let Ok(v) = row.try_get::<uuid::Uuid, _>(col) { return Some(v.to_string()); }
    if let Ok(v) = row.try_get::<i64, _>(col)         { return Some(v.to_string()); }
    if let Ok(v) = row.try_get::<i32, _>(col)         { return Some(v.to_string()); }
    if let Ok(v) = row.try_get::<i16, _>(col)         { return Some(v.to_string()); }
    if let Ok(v) = row.try_get::<String, _>(col)      { return Some(v); }
    None
}

// ─── Bind helpers ─────────────────────────────────────────────────────────────

use crate::ops::bind_sql_values;
use sqlx::{postgres::PgRow, Row};

async fn bind_and_fetch_all(pool: &PgPool, sql: &str, binds: Vec<SqlValue>) -> Result<Vec<PgRow>> {
    let q = sqlx::query(sql);
    let q = bind_sql_values(q, binds);
    q.fetch_all(pool).await.map_err(|e| anyhow::anyhow!("{}", e))
}

async fn bind_and_fetch_one(pool: &PgPool, sql: &str, binds: Vec<SqlValue>) -> Result<PgRow> {
    let q = sqlx::query(sql);
    let q = bind_sql_values(q, binds);
    q.fetch_one(pool).await.map_err(|e| anyhow::anyhow!("{}", e))
}
