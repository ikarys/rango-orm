use crate::ops::DEFAULT_QUERY_LIMIT;
use rango_orm_core::{
    ColumnRef, FilterColumn, ForeignKeyRef, FromRow, Model, ModelValues, OrderRef, SqlValue,
    quote_ident,
};
use sqlx::PgPool;
// ─── Bind helpers ─────────────────────────────────────────────────────────────

use super::*;

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
            raw_conditions: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add a raw WHERE clause — for use by backend-specific extensions only.
    /// Use `$__` as a placeholder; it will be replaced by `$N` at build time.
    pub(crate) fn raw_where(mut self, sql: String, binds: Vec<SqlValue>) -> Self {
        self.raw_conditions.push((sql, binds));
        self
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
        let inner = QueryBuilder::<M>::new(&self.pool);
        let inner = f(inner);
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
        let sub = sub(QueryBuilder::new(&self.pool));
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
        self,
        col: impl ColumnRef<M>,
        sub_col: impl ColumnRef<R>,
        sub: impl FnOnce(QueryBuilder<R>) -> QueryBuilder<R>,
    ) -> Self
    where
        R: Model + ModelValues + FromRow,
    {
        let head = format!(
            "{} IN (SELECT {} FROM {}",
            quote_ident(col.column_name()),
            quote_ident(sub_col.column_name()),
            quote_ident(R::table_name()),
        );
        let sub = sub(QueryBuilder::new(&self.pool));
        self.add_subquery(head, false, sub)
    }

    fn add_subquery<R>(mut self, head: String, has_where: bool, sub: QueryBuilder<R>) -> Self {
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

    // ── Ordering / pagination ─────────────────────────────────────────────────
    /// ORDER BY col ASC. Accepts `User::NAME`, `User::NAME.desc()`, or `"name"` / `"-name"` (DESC).
    pub fn order_by(mut self, col: impl OrderRef<M>) -> Self {
        let (name, descending) = col.order_parts();
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
    /// Use with care on large tables. The default cap is [`DEFAULT_QUERY_LIMIT`].
    pub fn unlimited(mut self) -> Self {
        self.explicit_limit = true;
        self
    }

    /// Returns the SQL that would be executed by `.all()`, for debugging.
    pub fn explain(&self) -> String {
        self.select_sql().0
    }

    /// The main `.all()` query: filtered rows of M, plus one LEFT JOIN per `select_related`.
    pub(super) fn select_sql(&self) -> (String, Vec<SqlValue>) {
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT * FROM {}", quote_ident(M::table_name()));
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        let order_by = if self.order_by.is_empty() {
            String::new()
        } else {
            format!(" ORDER BY {}", self.order_by.join(", "))
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
