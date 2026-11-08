use crate::ops::DEFAULT_QUERY_LIMIT;
use rango_orm_core::{
    ColumnRef, FilterColumn, ForeignKeyRef, FromRow, Model, ModelValues, OrderRef, SqlValue,
    quote_ident,
};
// ─── Bind helpers ─────────────────────────────────────────────────────────────

use super::*;

impl<'a, M> QueryBuilder<'a, M>
where
    M: Model + ModelValues + FromRow,
{
    pub fn new(conn: impl Into<Conn<'a>>) -> Self {
        Self {
            conn: conn.into(),
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
            raw_conditions: Vec::new(),
            invalid: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add a raw WHERE clause — for use by backend-specific extensions only.
    /// Use `$__` as a placeholder; it will be replaced by `$N` at build time.
    pub(crate) fn raw_where(mut self, sql: String, binds: Vec<SqlValue>) -> Self {
        self.raw_conditions.push((sql, binds));
        self
    }

    /// Remember the first unknown column of model `T`; reported when the query runs.
    pub(crate) fn check_col<T: Model>(&mut self, name: &str) {
        if self.invalid.is_none() {
            self.invalid = rango_orm_core::unknown_column::<T>(name);
        }
    }

    pub(super) fn ensure_valid(&self) -> Result<()> {
        match &self.invalid {
            Some(msg) => Err(rango_orm_core::Error::Invalid(msg.clone())),
            None => Ok(()),
        }
    }

    // ── Filter methods ────────────────────────────────────────────────────────
    /// WHERE col = value
    pub fn eq<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        let val = col.bind(val);
        self.add(col.column_name(), Op::Eq, Some(val), None)
    }

    /// WHERE col != value
    pub fn ne<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        let val = col.bind(val);
        self.add(col.column_name(), Op::Ne, Some(val), None)
    }

    /// WHERE col > value
    pub fn gt<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        let val = col.bind(val);
        self.add(col.column_name(), Op::Gt, Some(val), None)
    }

    /// WHERE col >= value
    pub fn gte<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        let val = col.bind(val);
        self.add(col.column_name(), Op::Gte, Some(val), None)
    }

    /// WHERE col < value
    pub fn lt<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        let val = col.bind(val);
        self.add(col.column_name(), Op::Lt, Some(val), None)
    }

    /// WHERE col <= value
    pub fn lte<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        let val = col.bind(val);
        self.add(col.column_name(), Op::Lte, Some(val), None)
    }

    /// WHERE col LIKE pattern
    /// Keyset pagination: rows with `col > value`, ordered by `col ASC` first.
    ///
    /// Unlike `offset`, cost does not grow with the page number. `col` should be
    /// unique and indexed (the primary key, for instance).
    ///
    /// # Example
    /// ```rust,ignore
    /// let mut last = None;
    /// loop {
    ///     let mut q = User::filter(&pool).limit(100);
    ///     if let Some(id) = last {
    ///         q = q.after(User::ID, id);
    ///     }
    ///     let page = q.order_by(User::ID).all().await?;
    ///     let Some(row) = page.last() else { break };
    ///     last = Some(row.id.clone());
    /// }
    /// ```
    pub fn after<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        let name = col.column_name().to_string();
        self.gt(col, val).order_first(&name)
    }

    /// Keyset pagination backwards: rows with `col < value`, ordered by `col DESC` first.
    pub fn before<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        let name = col.column_name().to_string();
        self.lt(col, val).order_first(&format!("-{name}"))
    }

    /// `order_by`, but as the highest-priority ordering.
    fn order_first(mut self, col: &str) -> Self {
        self = self.order_by(col);
        let last = self.order_by.pop().expect("order_by pushed one entry");
        // Drop an earlier `order_by` on the same column: no duplicate ORDER BY term.
        let prefix = format!("{} ", quote_ident(col.trim_start_matches('-')));
        self.order_by.retain(|o| !o.starts_with(&prefix));
        self.order_by.insert(0, last);
        self
    }

    pub fn like(self, col: impl ColumnRef<M>, pattern: impl Into<String>) -> Self {
        self.add(
            col.column_name(),
            Op::Like,
            Some(SqlValue::Text(pattern.into())),
            None,
        )
    }

    /// WHERE col ILIKE pattern (case-insensitive, PostgreSQL)
    pub fn ilike(self, col: impl ColumnRef<M>, pattern: impl Into<String>) -> Self {
        self.add(
            col.column_name(),
            Op::ILike,
            Some(SqlValue::Text(pattern.into())),
            None,
        )
    }

    /// WHERE col IN (values)
    pub fn in_values(self, col: impl ColumnRef<M>, vals: Vec<SqlValue>) -> Self {
        self.add(col.column_name(), Op::In, None, Some(vals))
    }

    /// WHERE col IS NULL
    pub fn is_null(self, col: impl ColumnRef<M>) -> Self {
        self.add(col.column_name(), Op::IsNull, None, None)
    }

    /// WHERE col IS NOT NULL
    pub fn is_not_null(self, col: impl ColumnRef<M>) -> Self {
        self.add(col.column_name(), Op::IsNotNull, None, None)
    }

    // ── Connector methods ─────────────────────────────────────────────────────
    /// Next condition uses OR
    pub fn or(mut self) -> Self {
        self.next_connector = Connector::Or;
        self
    }

    /// Next condition uses AND NOT
    #[allow(clippy::should_implement_trait)]
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
    /// ```rust,ignore
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
        let inner = QueryBuilder::<M>::new(Conn::Detached);
        let inner = f(inner);
        if self.invalid.is_none() {
            self.invalid = inner.invalid;
        }
        let connector = std::mem::replace(&mut self.next_connector, Connector::And);
        self.conditions.push(ConditionGroup {
            node: ConditionNode::Group(inner.conditions),
            connector,
        });
        self
    }

    // ── Subqueries ────────────────────────────────────────────────────────────
    /// Keep rows that have at least one `R` pointing at them through `fk_col`,
    /// among the rows kept by the filters `sub` adds:
    ///
    /// ```rust,ignore
    /// let authors = User::filter(&pool)
    ///     .where_exists(Article::AUTHOR_ID, |q| q.eq(Article::PUBLISHED, true))
    ///     .all().await?;
    /// // → WHERE EXISTS (SELECT 1 FROM "article" WHERE "article"."author_id" = "user"."id"
    /// //                 AND ("published" = $1))
    /// ```
    ///
    /// Chain after `.not()` for `NOT EXISTS`. Only `sub`'s filters are used
    /// (its ordering, limit and relations are ignored).
    pub fn where_exists<R>(
        self,
        fk_col: impl ForeignKeyRef<R, M>,
        sub: impl FnOnce(QueryBuilder<R>) -> QueryBuilder<R>,
    ) -> Self
    where
        R: Model + ModelValues + FromRow,
    {
        let outer = quote_ident(M::table_name());
        // Self-referencing (e.g. parent/child on one table): alias the inner table,
        // otherwise `"t"."id"` would resolve to the inner row.
        let (from, inner) = if R::table_name() == M::table_name() {
            (format!("{outer} AS \"__sub\""), "\"__sub\"".to_string())
        } else {
            let t = quote_ident(R::table_name());
            (t.clone(), t)
        };
        let head = format!(
            "EXISTS (SELECT 1 FROM {from} WHERE {inner}.{} = {outer}.{}",
            quote_ident(fk_col.column_name()),
            quote_ident(M::pk_column()),
        );
        let sub = sub(QueryBuilder::new(Conn::Detached));
        self.add_subquery(head, true, sub)
    }

    /// `col IN (SELECT sub_col FROM R WHERE <sub's filters>)`:
    ///
    /// ```rust,ignore
    /// let posts = Post::filter(&pool)
    ///     .in_subquery(Post::AUTHOR_ID, User::ID, |q| q.eq(User::ACTIVE, true))
    ///     .all().await?;
    /// ```
    ///
    /// Prefer `.not().where_exists(...)` over `.not().in_subquery(...)`: `NOT IN`
    /// matches nothing as soon as the subquery returns a NULL.
    pub fn in_subquery<R>(
        mut self,
        col: impl ColumnRef<M>,
        sub_col: impl ColumnRef<R>,
        sub: impl FnOnce(QueryBuilder<R>) -> QueryBuilder<R>,
    ) -> Self
    where
        R: Model + ModelValues + FromRow,
    {
        self.check_col::<M>(col.column_name());
        self.check_col::<R>(sub_col.column_name());
        let head = format!(
            "{} IN (SELECT {} FROM {}",
            quote_ident(col.column_name()),
            quote_ident(sub_col.column_name()),
            quote_ident(R::table_name()),
        );
        let sub = sub(QueryBuilder::new(Conn::Detached));
        self.add_subquery(head, false, sub)
    }

    fn add_subquery<R>(mut self, head: String, has_where: bool, sub: QueryBuilder<R>) -> Self {
        if self.invalid.is_none() {
            self.invalid = sub.invalid;
        }
        let connector = std::mem::replace(&mut self.next_connector, Connector::And);
        self.conditions.push(ConditionGroup {
            node: ConditionNode::Subquery(Box::new(Subquery {
                head,
                has_where,
                conditions: sub.conditions,
                raw_conditions: sub.raw_conditions,
            })),
            connector,
        });
        self
    }

    /// The explicit `order_by` calls, or the model's `ordering = [...]` when there are none.
    pub(crate) fn order_clauses(&self) -> Vec<String> {
        if !self.order_by.is_empty() {
            return self.order_by.clone();
        }
        M::schema()
            .ordering
            .iter()
            .map(|o| {
                let dir = if matches!(o.dir, rango_orm_core::OrderDir::Desc) {
                    "DESC"
                } else {
                    "ASC"
                };
                format!("{} {dir}", quote_ident(&o.column))
            })
            .collect()
    }

    // ── Ordering / pagination ─────────────────────────────────────────────────
    /// ORDER BY col ASC. Accepts `User::NAME`, `User::NAME.desc()`, or `"name"` / `"-name"` (DESC).
    pub fn order_by(mut self, col: impl OrderRef<M>) -> Self {
        let (name, descending) = col.order_parts();
        self.check_col::<M>(name);
        let dir = if descending { "DESC" } else { "ASC" };
        self.order_by.push(format!("{} {dir}", quote_ident(name)));
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
    /// Use with care on large tables. The default cap is `DEFAULT_QUERY_LIMIT` (1000 rows).
    pub fn unlimited(mut self) -> Self {
        self.explicit_limit = true;
        self
    }

    /// Warn when the implicit row cap may have hidden rows: no explicit `limit` /
    /// `unlimited()` and exactly `DEFAULT_QUERY_LIMIT` rows came back.
    pub(super) fn warn_if_capped(&self, rows: usize) {
        if !self.explicit_limit && rows as i64 == DEFAULT_QUERY_LIMIT {
            tracing::warn!(
                table = M::table_name(),
                cap = DEFAULT_QUERY_LIMIT,
                "query returned exactly the default row cap, results may be truncated; \
                 call .limit(n) to choose a cap or .unlimited() to fetch every row"
            );
        }
    }

    /// Returns the SQL that would be executed by `.all()`, for debugging.
    pub fn explain(&self) -> String {
        self.select_sql().0
    }

    /// The main `.all()` query: filtered rows of M, plus one LEFT JOIN per `select_related`.
    pub(super) fn select_sql(&self) -> (String, Vec<SqlValue>) {
        self.select_sql_cols("*")
    }

    /// `select_sql` with an explicit column list (already quoted) instead of `*`.
    pub(super) fn select_sql_cols(&self, cols: &str) -> (String, Vec<SqlValue>) {
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT {cols} FROM {}", quote_ident(M::table_name()));
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        let clauses = self.order_clauses();
        let order_by = if clauses.is_empty() {
            String::new()
        } else {
            format!(" ORDER BY {}", clauses.join(", "))
        };
        sql.push_str(&order_by);
        // Default row cap unless .limit() or .unlimited() was called.
        if !self.explicit_limit {
            sql.push_str(&format!(" LIMIT {}", DEFAULT_QUERY_LIMIT));
        } else if let Some(l) = self.limit {
            sql.push_str(&format!(" LIMIT {}", l));
        }
        if let Some(o) = self.offset {
            sql.push_str(&format!(" OFFSET {}", o));
        }
        if self.select_related.is_empty() {
            return (sql, binds);
        }

        // Filter, order and paginate M first, then join: LIMIT applies to M rows and
        // the filters keep their unqualified column names. R's columns get a
        // `__r{i}_` prefix so they cannot clash with M's.
        let mut select = String::from("SELECT \"__m\".*");
        let mut joins = String::new();
        for (i, spec) in self.select_related.iter().enumerate() {
            let alias = quote_ident(&format!("__r{i}"));
            for col in &spec.columns {
                select.push_str(&format!(
                    ", {alias}.{} AS {}",
                    quote_ident(col),
                    quote_ident(&format!("__r{i}_{col}"))
                ));
            }
            joins.push_str(&format!(
                " LEFT JOIN {} AS {alias} ON {alias}.{} = \"__m\".{}",
                quote_ident(spec.table_name),
                quote_ident(spec.pk_col),
                quote_ident(&spec.fk_col),
            ));
        }
        // The outer ORDER BY resolves to M's output columns (R's are prefixed).
        let sql = format!("{select} FROM ({sql}) AS \"__m\"{joins}{order_by}");
        (sql, binds)
    }

    // ── Internals ─────────────────────────────────────────────────────────────
    pub(super) fn add(
        mut self,
        col: &str,
        op: Op,
        value: Option<SqlValue>,
        values: Option<Vec<SqlValue>>,
    ) -> Self {
        self.check_col::<M>(col);
        let connector = std::mem::replace(&mut self.next_connector, Connector::And);
        self.conditions.push(ConditionGroup {
            node: ConditionNode::Single(Condition {
                column: col.to_string(),
                op,
                value,
                values,
            }),
            connector,
        });
        self
    }

    pub(super) fn build_where(&self) -> (String, Vec<SqlValue>) {
        let mut binds = Vec::new();
        let body = build_where_body(&self.conditions, &self.raw_conditions, &mut binds, &mut 1);
        if body.is_empty() {
            (body, binds)
        } else {
            (format!("WHERE {body}"), binds)
        }
    }
}
