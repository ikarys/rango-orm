use crate::ops::DEFAULT_QUERY_LIMIT;
use rango_core::{ColumnRef, FromRow, Model, ModelValues, OrderRef, SqlValue, quote_ident};
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
    pub fn eq(self, col: impl ColumnRef<M>, val: impl Into<SqlValue>) -> Self {
        self.add(col.column_name(), Op::Eq, Some(val.into()), None)
    }

    /// WHERE col != value
    pub fn ne(self, col: impl ColumnRef<M>, val: impl Into<SqlValue>) -> Self {
        self.add(col.column_name(), Op::Ne, Some(val.into()), None)
    }

    /// WHERE col > value
    pub fn gt(self, col: impl ColumnRef<M>, val: impl Into<SqlValue>) -> Self {
        self.add(col.column_name(), Op::Gt, Some(val.into()), None)
    }

    /// WHERE col >= value
    pub fn gte(self, col: impl ColumnRef<M>, val: impl Into<SqlValue>) -> Self {
        self.add(col.column_name(), Op::Gte, Some(val.into()), None)
    }

    /// WHERE col < value
    pub fn lt(self, col: impl ColumnRef<M>, val: impl Into<SqlValue>) -> Self {
        self.add(col.column_name(), Op::Lt, Some(val.into()), None)
    }

    /// WHERE col <= value
    pub fn lte(self, col: impl ColumnRef<M>, val: impl Into<SqlValue>) -> Self {
        self.add(col.column_name(), Op::Lte, Some(val.into()), None)
    }

    /// WHERE col LIKE pattern
    pub fn like(self, col: impl ColumnRef<M>, pattern: impl Into<String>) -> Self {
        self.add(col.column_name(), Op::Like, Some(SqlValue::Text(pattern.into())), None)
    }

    /// WHERE col ILIKE pattern (case-insensitive, PostgreSQL)
    pub fn ilike(self, col: impl ColumnRef<M>, pattern: impl Into<String>) -> Self {
        self.add(col.column_name(), Op::ILike, Some(SqlValue::Text(pattern.into())), None)
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
        let table = M::table_name();
        let (where_clause, _) = self.build_where();
        let mut sql = format!("SELECT * FROM {}", quote_ident(table));
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        if !self.order_by.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", self.order_by.join(", ")));
        }
        if let Some(l) = self.limit {
            sql.push_str(&format!(" LIMIT {}", l));
        } else if !self.explicit_limit {
            sql.push_str(&format!(" LIMIT {}", DEFAULT_QUERY_LIMIT));
        }
        if let Some(o) = self.offset {
            sql.push_str(&format!(" OFFSET {}", o));
        }
        sql
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
        let has_conditions = !self.conditions.is_empty();
        let has_raw = !self.raw_conditions.is_empty();

        if !has_conditions && !has_raw {
            return (String::new(), Vec::new());
        }

        let mut binds = Vec::new();
        let mut idx = 1usize;
        let mut parts: Vec<String> = Vec::new();

        if has_conditions {
            parts.push(build_conditions(&self.conditions, &mut binds, &mut idx));
        }

        // Append raw conditions — replace $__ with the current $N
        for (sql, raw_binds) in &self.raw_conditions {
            let mut fragment = sql.clone();
            for bind in raw_binds {
                let placeholder = format!("${}", idx);
                fragment = fragment.replacen("$__", &placeholder, 1);
                binds.push(bind.clone());
                idx += 1;
            }
            parts.push(fragment);
        }

        (format!("WHERE {}", parts.join(" AND ")), binds)
    }
}
