//! DB-free query-builder state shared by the backends: the condition tree and every
//! SQL string a `QueryBuilder` produces. Backends keep only binding and execution.

use crate::related::{CollectVecFn, FromRowFn, collect_vec_erased, erased_from_row, m2m_pivot};
use crate::{
    ColumnRef, Error, FilterColumn, ForeignKeyRef, FromRow, Model, ModelValues, OrderDir, OrderRef,
    PrefetchKind, PrefetchRef, Result, SqlValue,
};
use std::any::TypeId;
use std::marker::PhantomData;

/// Rows returned by a query without an explicit `limit` / `unlimited()`.
pub const DEFAULT_QUERY_LIMIT: i64 = 1000;

/// The SQL dialect a [`Filter`] renders for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Postgres,
    Sqlite,
    Mysql,
}

impl Dialect {
    /// Quote an identifier: backticks on MySQL, `"` elsewhere.
    pub fn quote_ident(self, ident: &str) -> String {
        match self {
            Dialect::Mysql => crate::quote_ident_mysql(ident),
            _ => crate::quote_ident(ident),
        }
    }

    /// The next bind placeholder: `$n` on Postgres, `?` elsewhere.
    fn placeholder(self, idx: &mut usize) -> String {
        let p = match self {
            Dialect::Postgres => format!("${idx}"),
            _ => "?".to_string(),
        };
        *idx += 1;
        p
    }

    /// `SUM` / `AVG` returning a float (`MIN` / `MAX` keep the column type).
    fn aggregate(self, func: &str, col: &str) -> String {
        match (self, func) {
            (Dialect::Postgres, "SUM" | "AVG") => format!("{func}({col})::float8"),
            (Dialect::Mysql, "SUM" | "AVG") => format!("CAST({func}({col}) AS DOUBLE)"),
            (Dialect::Sqlite, "SUM") => format!("CAST(SUM({col}) AS REAL)"),
            _ => format!("{func}({col})"),
        }
    }
}

#[derive(Debug, Clone)]
enum Op {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    Like,
    ILike,
    In,
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone)]
struct Condition {
    column: String,
    op: Op,
    value: Option<SqlValue>,
    values: Option<Vec<SqlValue>>, // for IN
}

#[derive(Debug, Clone)]
enum Connector {
    And,
    Or,
    AndNot,
    OrNot,
    Xor,
}

#[derive(Debug, Clone)]
enum ConditionNode {
    Single(Condition),
    Group(Vec<ConditionGroup>),
    Subquery(Box<Subquery>),
}

/// `EXISTS (...)` / `col IN (...)` built from another builder's filters.
#[derive(Debug, Clone)]
struct Subquery {
    /// SQL up to the subquery's filters, e.g. `"col" IN (SELECT "x" FROM "t"`.
    head: String,
    /// True when `head` already ends with a WHERE clause (correlated EXISTS).
    has_where: bool,
    conditions: Vec<ConditionGroup>,
    raw_conditions: Vec<(String, Vec<SqlValue>)>,
}

#[derive(Debug, Clone)]
struct ConditionGroup {
    node: ConditionNode,
    connector: Connector,
}

/// A `.select_related(fk_col)` call: `m.fk_col = r.pk`, loaded with a LEFT JOIN.
#[doc(hidden)]
pub struct SelectRelatedSpec {
    /// Column on M's table containing the FK to R.
    pub fk_col: String,
    /// `TypeId::of::<R>()`, the `WithRelated` cache key.
    pub type_id: TypeId,
    pub table_name: &'static str,
    /// R's primary key column name.
    pub pk_col: &'static str,
    /// R's columns, selected as `"__r{i}"."col" AS "__r{i}_col"`.
    pub columns: Vec<String>,
    /// Type-erased `R::from_row`.
    pub from_row: FromRowFn,
}

/// A `.prefetch_related(..)` call: a reverse FK or a M2M field.
#[doc(hidden)]
pub struct PrefetchSpec {
    /// SQL up to and including the key expression, without the `= ANY(..)` / `IN (..)` part.
    pub select: String,
    /// Result column holding M's primary key value in each fetched row.
    pub key_col: String,
    /// `TypeId::of::<Vec<R>>()`, the `WithRelated` cache key.
    pub vec_type_id: TypeId,
    pub from_row: FromRowFn,
    /// Collects `Vec<Box<dyn Any>>` into a `Box<dyn Any>` containing `Vec<R>`.
    pub collect_vec: CollectVecFn,
}

/// Filters, ordering, pagination and relation specs of one query, plus the SQL they render to.
/// Backends wrap it in their `QueryBuilder` (see `impl_filter_methods!`) with a connection.
#[doc(hidden)]
pub struct Filter<M> {
    dialect: Dialect,
    conditions: Vec<ConditionGroup>,
    order_by: Vec<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    explicit_limit: bool,
    next_connector: Connector,
    select_related: Vec<SelectRelatedSpec>,
    prefetch_related: Vec<PrefetchSpec>,
    /// Raw WHERE clauses from backend extensions; on Postgres `$__` becomes `$N`.
    raw_conditions: Vec<(String, Vec<SqlValue>)>,
    /// First unknown column met while building; returned as `Error::Invalid` on execution.
    invalid: Option<String>,
    _phantom: PhantomData<fn() -> M>,
}

impl<M: Model + ModelValues> Filter<M> {
    pub fn new(dialect: Dialect) -> Self {
        Self {
            dialect,
            conditions: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
            explicit_limit: false,
            next_connector: Connector::And,
            select_related: Vec::new(),
            prefetch_related: Vec::new(),
            raw_conditions: Vec::new(),
            invalid: None,
            _phantom: PhantomData,
        }
    }

    fn q(&self, ident: &str) -> String {
        self.dialect.quote_ident(ident)
    }

    pub fn raw_where(mut self, sql: String, binds: Vec<SqlValue>) -> Self {
        self.raw_conditions.push((sql, binds));
        self
    }

    /// Remember the first unknown column of model `T`; reported when the query runs.
    pub fn check_col<T: Model>(&mut self, name: &str) {
        if self.invalid.is_none() {
            self.invalid = crate::unknown_column::<T>(name);
        }
    }

    pub fn ensure_valid(&self) -> Result<()> {
        match &self.invalid {
            Some(msg) => Err(Error::Invalid(msg.clone())),
            None => Ok(()),
        }
    }

    pub fn select_related_specs(&self) -> &[SelectRelatedSpec] {
        &self.select_related
    }

    pub fn prefetch_specs(&self) -> &[PrefetchSpec] {
        &self.prefetch_related
    }

    /// True when the implicit row cap may have hidden rows: no explicit `limit` /
    /// `unlimited()` and exactly `DEFAULT_QUERY_LIMIT` rows came back.
    pub fn is_capped(&self, rows: usize) -> bool {
        !self.explicit_limit && rows as i64 == DEFAULT_QUERY_LIMIT
    }

    // ── Conditions ────────────────────────────────────────────────────────────
    fn push_condition(
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

    fn cmp<V>(self, col: impl FilterColumn<M, V>, op: Op, val: V) -> Self {
        let val = col.bind(val);
        self.push_condition(col.column_name(), op, Some(val), None)
    }

    pub fn eq<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        self.cmp(col, Op::Eq, val)
    }
    pub fn ne<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        self.cmp(col, Op::Ne, val)
    }
    pub fn gt<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        self.cmp(col, Op::Gt, val)
    }
    pub fn gte<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        self.cmp(col, Op::Gte, val)
    }
    pub fn lt<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        self.cmp(col, Op::Lt, val)
    }
    pub fn lte<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        self.cmp(col, Op::Lte, val)
    }

    pub fn after<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        let name = col.column_name().to_string();
        self.gt(col, val).order_first(&name)
    }

    pub fn before<V>(self, col: impl FilterColumn<M, V>, val: V) -> Self {
        let name = col.column_name().to_string();
        self.lt(col, val).order_first(&format!("-{name}"))
    }

    /// `order_by`, but as the highest-priority ordering.
    fn order_first(mut self, col: &str) -> Self {
        self = self.order_by(col);
        let last = self.order_by.pop().expect("order_by pushed one entry");
        // Drop an earlier `order_by` on the same column: no duplicate ORDER BY term.
        let prefix = format!("{} ", self.q(col.trim_start_matches('-')));
        self.order_by.retain(|o| !o.starts_with(&prefix));
        self.order_by.insert(0, last);
        self
    }

    pub fn like(self, col: impl ColumnRef<M>, pattern: String) -> Self {
        self.push_condition(
            col.column_name(),
            Op::Like,
            Some(SqlValue::Text(pattern)),
            None,
        )
    }

    pub fn ilike(self, col: impl ColumnRef<M>, pattern: String) -> Self {
        self.push_condition(
            col.column_name(),
            Op::ILike,
            Some(SqlValue::Text(pattern)),
            None,
        )
    }

    pub fn in_values(self, col: impl ColumnRef<M>, vals: Vec<SqlValue>) -> Self {
        self.push_condition(col.column_name(), Op::In, None, Some(vals))
    }

    pub fn is_null(self, col: impl ColumnRef<M>) -> Self {
        self.push_condition(col.column_name(), Op::IsNull, None, None)
    }

    pub fn is_not_null(self, col: impl ColumnRef<M>) -> Self {
        self.push_condition(col.column_name(), Op::IsNotNull, None, None)
    }

    pub fn or(mut self) -> Self {
        self.next_connector = Connector::Or;
        self
    }

    #[allow(clippy::should_implement_trait)]
    pub fn not(mut self) -> Self {
        self.next_connector = Connector::AndNot;
        self
    }

    pub fn or_not(mut self) -> Self {
        self.next_connector = Connector::OrNot;
        self
    }

    pub fn xor(mut self) -> Self {
        self.next_connector = Connector::Xor;
        self
    }

    pub fn group(mut self, f: impl FnOnce(Filter<M>) -> Filter<M>) -> Self {
        let inner = f(Filter::new(self.dialect));
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

    pub fn where_exists<R: Model + ModelValues>(
        self,
        fk_col: impl ForeignKeyRef<R, M>,
        sub: impl FnOnce(Filter<R>) -> Filter<R>,
    ) -> Self {
        let outer = self.q(M::table_name());
        // Self-referencing (e.g. parent/child on one table): alias the inner table,
        // otherwise `"t"."id"` would resolve to the inner row.
        let (from, inner) = if R::table_name() == M::table_name() {
            let alias = self.q("__sub");
            (format!("{outer} AS {alias}"), alias)
        } else {
            let t = self.q(R::table_name());
            (t.clone(), t)
        };
        let head = format!(
            "EXISTS (SELECT 1 FROM {from} WHERE {inner}.{} = {outer}.{}",
            self.q(fk_col.column_name()),
            self.q(M::pk_column()),
        );
        let sub = sub(Filter::new(self.dialect));
        self.add_subquery(head, true, sub)
    }

    pub fn in_subquery<R: Model + ModelValues>(
        mut self,
        col: impl ColumnRef<M>,
        sub_col: impl ColumnRef<R>,
        sub: impl FnOnce(Filter<R>) -> Filter<R>,
    ) -> Self {
        self.check_col::<M>(col.column_name());
        self.check_col::<R>(sub_col.column_name());
        let head = format!(
            "{} IN (SELECT {} FROM {}",
            self.q(col.column_name()),
            self.q(sub_col.column_name()),
            self.q(R::table_name()),
        );
        let sub = sub(Filter::new(self.dialect));
        self.add_subquery(head, false, sub)
    }

    fn add_subquery<R>(mut self, head: String, has_where: bool, sub: Filter<R>) -> Self {
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

    // ── Ordering / pagination ─────────────────────────────────────────────────
    pub fn order_by(mut self, col: impl OrderRef<M>) -> Self {
        let (name, descending) = col.order_parts();
        self.check_col::<M>(name);
        let dir = if descending { "DESC" } else { "ASC" };
        self.order_by.push(format!("{} {dir}", self.q(name)));
        self
    }

    /// Set a LIMIT and mark it explicit (suppresses the automatic row cap).
    pub fn limit(mut self, n: i64) -> Self {
        self.limit = Some(n);
        self.explicit_limit = true;
        self
    }

    pub fn offset(mut self, n: i64) -> Self {
        self.offset = Some(n);
        self
    }

    pub fn unlimited(mut self) -> Self {
        self.explicit_limit = true;
        self
    }

    // ── Relations ─────────────────────────────────────────────────────────────
    pub fn select_related<R>(mut self, fk_col: impl ForeignKeyRef<M, R>) -> Self
    where
        R: Model + ModelValues + FromRow + Send + Sync + 'static,
    {
        self.select_related.push(SelectRelatedSpec {
            fk_col: fk_col.column_name().to_string(),
            type_id: TypeId::of::<R>(),
            table_name: R::table_name(),
            pk_col: R::pk_column(),
            columns: R::schema().columns.into_iter().map(|c| c.name).collect(),
            from_row: erased_from_row::<R>,
        });
        self
    }

    pub fn prefetch_related<R>(mut self, target: impl PrefetchRef<M, R>) -> Self
    where
        R: Model + ModelValues + FromRow + Send + Sync + 'static,
    {
        let (select, key_col) = match target.prefetch_kind() {
            PrefetchKind::ReverseFk(col) => (
                format!(
                    "SELECT * FROM {} WHERE {}",
                    self.q(R::table_name()),
                    self.q(col),
                ),
                col.to_string(),
            ),
            PrefetchKind::ManyToMany => {
                let (pivot, from_col, to_col) = m2m_pivot(M::table_name(), R::table_name());
                let (p, t) = (self.q("__p"), self.q("__t"));
                let select = format!(
                    "SELECT {p}.{from} AS {key}, {t}.* FROM {to} AS {t} \
                     INNER JOIN {pivot} AS {p} ON {t}.{pk} = {p}.{to_col} WHERE {p}.{from}",
                    from = self.q(&from_col),
                    key = self.q("__m2m_key"),
                    to = self.q(R::table_name()),
                    pivot = self.q(&pivot),
                    pk = self.q(R::pk_column()),
                    to_col = self.q(&to_col),
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

    // ── SQL ───────────────────────────────────────────────────────────────────
    /// The explicit `order_by` calls, or the model's `ordering = [...]` when there are none.
    fn order_clauses(&self) -> Vec<String> {
        if !self.order_by.is_empty() {
            return self.order_by.clone();
        }
        M::schema()
            .ordering
            .iter()
            .map(|o| {
                let dir = if matches!(o.dir, OrderDir::Desc) {
                    "DESC"
                } else {
                    "ASC"
                };
                format!("{} {dir}", self.q(&o.column))
            })
            .collect()
    }

    /// `WHERE ...` (empty when there is no filter) and its binds, in placeholder order.
    pub fn build_where(&self) -> (String, Vec<SqlValue>) {
        let mut binds = Vec::new();
        let body = self.where_body(&self.conditions, &self.raw_conditions, &mut binds, &mut 1);
        if body.is_empty() {
            (body, binds)
        } else {
            (format!("WHERE {body}"), binds)
        }
    }

    fn conditions_sql(
        &self,
        groups: &[ConditionGroup],
        binds: &mut Vec<SqlValue>,
        idx: &mut usize,
    ) -> String {
        let mut parts = Vec::new();
        for (i, cg) in groups.iter().enumerate() {
            let expr = match &cg.node {
                ConditionNode::Single(c) => self.condition_sql(c, binds, idx),
                ConditionNode::Group(inner) => {
                    format!("({})", self.conditions_sql(inner, binds, idx))
                }
                ConditionNode::Subquery(sub) => {
                    let body = self.where_body(&sub.conditions, &sub.raw_conditions, binds, idx);
                    match (body.is_empty(), sub.has_where) {
                        (true, _) => format!("{})", sub.head),
                        (false, true) => format!("{} AND ({}))", sub.head, body),
                        (false, false) => format!("{} WHERE {})", sub.head, body),
                    }
                }
            };
            parts.push(if i == 0 {
                match &cg.connector {
                    Connector::AndNot | Connector::OrNot => format!("NOT {expr}"),
                    _ => expr,
                }
            } else {
                match &cg.connector {
                    Connector::And => format!("AND {expr}"),
                    Connector::Or => format!("OR {expr}"),
                    Connector::AndNot => format!("AND NOT {expr}"),
                    Connector::OrNot => format!("OR NOT {expr}"),
                    // SQLite has no XOR.
                    Connector::Xor if self.dialect == Dialect::Sqlite => format!("OR {expr}"),
                    Connector::Xor => format!("XOR {expr}"),
                }
            });
        }
        parts.join(" ")
    }

    /// Filters joined with AND, without the `WHERE` keyword. Empty when there is no filter.
    fn where_body(
        &self,
        conditions: &[ConditionGroup],
        raw_conditions: &[(String, Vec<SqlValue>)],
        binds: &mut Vec<SqlValue>,
        idx: &mut usize,
    ) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !conditions.is_empty() {
            let expr = self.conditions_sql(conditions, binds, idx);
            // Parenthesize so an OR chain does not swallow the raw conditions after it.
            parts.push(if raw_conditions.is_empty() {
                expr
            } else {
                format!("({expr})")
            });
        }
        for (sql, raw_binds) in raw_conditions {
            let mut fragment = sql.clone();
            for bind in raw_binds {
                let p = self.dialect.placeholder(idx);
                if self.dialect == Dialect::Postgres {
                    fragment = fragment.replacen("$__", &p, 1);
                }
                binds.push(bind.clone());
            }
            parts.push(fragment);
        }
        parts.join(" AND ")
    }

    fn condition_sql(&self, c: &Condition, binds: &mut Vec<SqlValue>, idx: &mut usize) -> String {
        let col = self.q(&c.column);
        match &c.op {
            Op::IsNull => format!("{col} IS NULL"),
            Op::IsNotNull => format!("{col} IS NOT NULL"),
            Op::In => {
                let vals = c.values.as_ref().unwrap();
                if vals.is_empty() {
                    // `IN ()` is a syntax error; an empty set matches nothing.
                    return "(1 = 0)".to_string();
                }
                let placeholders: Vec<String> =
                    vals.iter().map(|_| self.dialect.placeholder(idx)).collect();
                binds.extend(vals.clone());
                format!("{col} IN ({})", placeholders.join(", "))
            }
            op => {
                let p = self.dialect.placeholder(idx);
                binds.push(c.value.clone().unwrap());
                let op_str = match (op, self.dialect) {
                    // MySQL: case-sensitive like Postgres, whatever the column collation.
                    (Op::Like, Dialect::Mysql) => {
                        return format!("{col} COLLATE utf8mb4_bin LIKE {p}");
                    }
                    (Op::ILike, Dialect::Mysql) => {
                        return format!("LOWER({col}) LIKE LOWER({p})");
                    }
                    (Op::Eq, _) => "=",
                    (Op::Ne, _) => "!=",
                    (Op::Gt, _) => ">",
                    (Op::Gte, _) => ">=",
                    (Op::Lt, _) => "<",
                    (Op::Lte, _) => "<=",
                    (Op::ILike, Dialect::Postgres) => "ILIKE",
                    // SQLite LIKE is case-insensitive by default.
                    (Op::Like | Op::ILike, _) => "LIKE",
                    _ => unreachable!(),
                };
                format!("{col} {op_str} {p}")
            }
        }
    }

    /// `SELECT <cols> FROM t WHERE .. ORDER BY .. LIMIT .. OFFSET ..`, plus the ORDER BY clause alone.
    fn plain_select(&self, cols: &str) -> (String, String, Vec<SqlValue>) {
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT {cols} FROM {}", self.q(M::table_name()));
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
            sql.push_str(&format!(" LIMIT {DEFAULT_QUERY_LIMIT}"));
        } else if let Some(l) = self.limit {
            sql.push_str(&format!(" LIMIT {l}"));
        }
        if let Some(o) = self.offset {
            sql.push_str(&format!(" OFFSET {o}"));
        }
        (sql, order_by, binds)
    }

    /// `SELECT` of the given plain column names, no relations.
    pub fn values_sql(&self, cols: &[&str]) -> (String, Vec<SqlValue>) {
        let list: Vec<String> = cols.iter().map(|c| self.q(c)).collect();
        let (sql, _, binds) = self.plain_select(&list.join(", "));
        (sql, binds)
    }

    /// The main `.all()` query: filtered rows of M, plus one LEFT JOIN per `select_related`.
    pub fn select_sql(&self) -> (String, Vec<SqlValue>) {
        self.select_sql_cols("*")
    }

    /// `select_sql` with an explicit column list (already quoted) instead of `*`.
    pub fn select_sql_cols(&self, cols: &str) -> (String, Vec<SqlValue>) {
        let (sql, order_by, binds) = self.plain_select(cols);
        if self.select_related.is_empty() {
            return (sql, binds);
        }

        // Filter, order and paginate M first, then join: LIMIT applies to M rows and
        // the filters keep their unqualified column names. R's columns get a
        // `__r{i}_` prefix so they cannot clash with M's.
        let m = self.q("__m");
        let mut select = format!("SELECT {m}.*");
        let mut joins = String::new();
        for (i, spec) in self.select_related.iter().enumerate() {
            let alias = self.q(&format!("__r{i}"));
            for col in &spec.columns {
                select.push_str(&format!(
                    ", {alias}.{} AS {}",
                    self.q(col),
                    self.q(&format!("__r{i}_{col}"))
                ));
            }
            joins.push_str(&format!(
                " LEFT JOIN {} AS {alias} ON {alias}.{} = {m}.{}",
                self.q(spec.table_name),
                self.q(spec.pk_col),
                self.q(&spec.fk_col),
            ));
        }
        // The outer ORDER BY resolves to M's output columns (R's are prefixed).
        (
            format!("{select} FROM ({sql}) AS {m}{joins}{order_by}"),
            binds,
        )
    }

    /// `SELECT COUNT(*) FROM t WHERE ..`
    pub fn count_sql(&self) -> (String, Vec<SqlValue>) {
        self.aggregate_expr_sql("COUNT(*)".to_string())
    }

    /// `SELECT EXISTS(SELECT 1 FROM t WHERE ..)`: stops at the first matching row.
    pub fn exists_sql(&self) -> (String, Vec<SqlValue>) {
        let (inner, binds) = self.aggregate_expr_sql("1".to_string());
        (format!("SELECT EXISTS({inner})"), binds)
    }

    /// `SELECT <func>(col) FROM t WHERE ..` for `SUM`, `AVG`, `MIN`, `MAX` (floats for `SUM` / `AVG`).
    pub fn aggregate_sql(&self, func: &str, col: &str) -> (String, Vec<SqlValue>) {
        self.aggregate_expr_sql(self.dialect.aggregate(func, &self.q(col)))
    }

    fn aggregate_expr_sql(&self, expr: String) -> (String, Vec<SqlValue>) {
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT {expr} FROM {}", self.q(M::table_name()));
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        (sql, binds)
    }
}

/// Expands, inside a backend's `query_builder`, the chainable filter methods of
/// `QueryBuilder<'a, M>` as thin delegations to its `f: Filter<M>` field.
/// The backend provides `QueryBuilder { conn, f }`, `Conn::Detached` and
/// `fn map(self, impl FnOnce(Filter<M>) -> Filter<M>) -> Self`.
#[doc(hidden)]
#[macro_export]
macro_rules! impl_filter_methods {
    () => {
        impl<'a, M> QueryBuilder<'a, M>
        where
            M: $crate::Model + $crate::ModelValues + $crate::FromRow,
        {
            /// WHERE col = value
            pub fn eq<V>(self, col: impl $crate::FilterColumn<M, V>, val: V) -> Self {
                self.map(|f| f.eq(col, val))
            }

            /// WHERE col != value
            pub fn ne<V>(self, col: impl $crate::FilterColumn<M, V>, val: V) -> Self {
                self.map(|f| f.ne(col, val))
            }

            /// WHERE col > value
            pub fn gt<V>(self, col: impl $crate::FilterColumn<M, V>, val: V) -> Self {
                self.map(|f| f.gt(col, val))
            }

            /// WHERE col >= value
            pub fn gte<V>(self, col: impl $crate::FilterColumn<M, V>, val: V) -> Self {
                self.map(|f| f.gte(col, val))
            }

            /// WHERE col < value
            pub fn lt<V>(self, col: impl $crate::FilterColumn<M, V>, val: V) -> Self {
                self.map(|f| f.lt(col, val))
            }

            /// WHERE col <= value
            pub fn lte<V>(self, col: impl $crate::FilterColumn<M, V>, val: V) -> Self {
                self.map(|f| f.lte(col, val))
            }

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
            pub fn after<V>(self, col: impl $crate::FilterColumn<M, V>, val: V) -> Self {
                self.map(|f| f.after(col, val))
            }

            /// Keyset pagination backwards: rows with `col < value`, ordered by `col DESC` first.
            pub fn before<V>(self, col: impl $crate::FilterColumn<M, V>, val: V) -> Self {
                self.map(|f| f.before(col, val))
            }

            /// WHERE col LIKE pattern
            pub fn like(self, col: impl $crate::ColumnRef<M>, pattern: impl Into<String>) -> Self {
                self.map(|f| f.like(col, pattern.into()))
            }

            /// WHERE col ILIKE pattern (case-insensitive)
            pub fn ilike(self, col: impl $crate::ColumnRef<M>, pattern: impl Into<String>) -> Self {
                self.map(|f| f.ilike(col, pattern.into()))
            }

            /// WHERE col IN (values)
            pub fn in_values(
                self,
                col: impl $crate::ColumnRef<M>,
                vals: Vec<$crate::SqlValue>,
            ) -> Self {
                self.map(|f| f.in_values(col, vals))
            }

            /// WHERE col IS NULL
            pub fn is_null(self, col: impl $crate::ColumnRef<M>) -> Self {
                self.map(|f| f.is_null(col))
            }

            /// WHERE col IS NOT NULL
            pub fn is_not_null(self, col: impl $crate::ColumnRef<M>) -> Self {
                self.map(|f| f.is_not_null(col))
            }

            /// Next condition uses OR
            pub fn or(self) -> Self {
                self.map(|f| f.or())
            }

            /// Next condition uses AND NOT
            #[allow(clippy::should_implement_trait)]
            pub fn not(self) -> Self {
                self.map(|f| f.not())
            }

            /// Next condition uses OR NOT
            pub fn or_not(self) -> Self {
                self.map(|f| f.or_not())
            }

            /// Next condition uses XOR (rendered as OR on SQLite, which has no XOR)
            pub fn xor(self) -> Self {
                self.map(|f| f.xor())
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
            pub fn group<F>(self, g: F) -> Self
            where
                F: FnOnce(QueryBuilder<M>) -> QueryBuilder<M>,
            {
                self.map(|f| f.group(|inner| g(QueryBuilder::detached(inner)).f))
            }

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
                fk_col: impl $crate::ForeignKeyRef<R, M>,
                sub: impl FnOnce(QueryBuilder<R>) -> QueryBuilder<R>,
            ) -> Self
            where
                R: $crate::Model + $crate::ModelValues + $crate::FromRow,
            {
                self.map(|f| f.where_exists(fk_col, |s| sub(QueryBuilder::detached(s)).f))
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
                col: impl $crate::ColumnRef<M>,
                sub_col: impl $crate::ColumnRef<R>,
                sub: impl FnOnce(QueryBuilder<R>) -> QueryBuilder<R>,
            ) -> Self
            where
                R: $crate::Model + $crate::ModelValues + $crate::FromRow,
            {
                self.map(|f| f.in_subquery(col, sub_col, |s| sub(QueryBuilder::detached(s)).f))
            }

            /// ORDER BY col ASC. Accepts `User::NAME`, `User::NAME.desc()`, or `"name"` / `"-name"` (DESC).
            pub fn order_by(self, col: impl $crate::OrderRef<M>) -> Self {
                self.map(|f| f.order_by(col))
            }

            /// Apply a LIMIT and mark it as explicit (suppresses the automatic 1000-row cap).
            pub fn limit(self, n: i64) -> Self {
                self.map(|f| f.limit(n))
            }

            pub fn offset(self, n: i64) -> Self {
                self.map(|f| f.offset(n))
            }

            /// Disable the default row cap — returns all rows with no LIMIT clause.
            ///
            /// Use with care on large tables. The default cap is `DEFAULT_QUERY_LIMIT` (1000 rows).
            pub fn unlimited(self) -> Self {
                self.map(|f| f.unlimited())
            }

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
            pub fn select_related<R>(self, fk_col: impl $crate::ForeignKeyRef<M, R>) -> Self
            where
                R: $crate::Model + $crate::ModelValues + $crate::FromRow + Send + Sync + 'static,
            {
                self.map(|f| f.select_related(fk_col))
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
            pub fn prefetch_related<R>(self, target: impl $crate::PrefetchRef<M, R>) -> Self
            where
                R: $crate::Model + $crate::ModelValues + $crate::FromRow + Send + Sync + 'static,
            {
                self.map(|f| f.prefetch_related(target))
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RangoRow, RowError, TableSchema};

    #[derive(Debug)]
    struct T;

    impl crate::ModelHooks for T {}

    impl Model for T {
        type Pk = uuid::Uuid;
        fn table_name() -> &'static str {
            "test_model"
        }
        fn schema() -> TableSchema {
            TableSchema::default()
        }
    }

    impl ModelValues for T {
        fn field_values(&self) -> Vec<(&'static str, SqlValue)> {
            Vec::new()
        }
        fn pk_value(&self) -> SqlValue {
            SqlValue::BigInt(1)
        }
        fn pk_column() -> &'static str {
            "id"
        }
    }

    impl FromRow for T {
        fn from_row(_row: &dyn RangoRow) -> std::result::Result<Self, RowError> {
            Ok(Self)
        }
    }

    fn nested(d: Dialect) -> Filter<T> {
        Filter::<T>::new(d)
            .eq("a", 1i32)
            .or_not()
            .where_exists::<T>("parent_id", |q| q.eq("b", 2i32))
            .in_subquery::<T>("c", "id", |q| q)
            .eq("d", 3i32)
    }

    #[test]
    fn exists_sql_short_circuits() {
        let (sql, binds) = Filter::<T>::new(Dialect::Postgres)
            .eq("a", 1i32)
            .exists_sql();
        assert_eq!(
            sql,
            "SELECT EXISTS(SELECT 1 FROM \"test_model\" WHERE \"a\" = $1)"
        );
        assert_eq!(binds.len(), 1);
    }

    #[test]
    fn subqueries_number_their_placeholders_in_order() {
        let (sql, binds) = nested(Dialect::Postgres).select_sql();
        assert_eq!(
            sql,
            "SELECT * FROM \"test_model\" WHERE \"a\" = $1 \
             OR NOT EXISTS (SELECT 1 FROM \"test_model\" AS \"__sub\" \
             WHERE \"__sub\".\"parent_id\" = \"test_model\".\"id\" AND (\"b\" = $2)) \
             AND \"c\" IN (SELECT \"id\" FROM \"test_model\") AND \"d\" = $3 LIMIT 1000"
        );
        assert_eq!(binds.len(), 3);
    }

    #[test]
    fn question_mark_dialects_quote_their_own_way() {
        let (sql, _) = nested(Dialect::Sqlite).build_where();
        assert_eq!(
            sql,
            "WHERE \"a\" = ? OR NOT EXISTS (SELECT 1 FROM \"test_model\" AS \"__sub\" \
             WHERE \"__sub\".\"parent_id\" = \"test_model\".\"id\" AND (\"b\" = ?)) \
             AND \"c\" IN (SELECT \"id\" FROM \"test_model\") AND \"d\" = ?"
        );
        let (sql, _) = nested(Dialect::Mysql).build_where();
        assert_eq!(
            sql,
            "WHERE `a` = ? OR NOT EXISTS (SELECT 1 FROM `test_model` AS `__sub` \
             WHERE `__sub`.`parent_id` = `test_model`.`id` AND (`b` = ?)) \
             AND `c` IN (SELECT `id` FROM `test_model`) AND `d` = ?"
        );
    }

    #[test]
    fn select_related_joins_after_paginating() {
        let (sql, _) = Filter::<T>::new(Dialect::Postgres)
            .select_related::<T>("parent_id")
            .order_by("-a")
            .limit(5)
            .select_sql();
        assert_eq!(
            sql,
            "SELECT \"__m\".* FROM (SELECT * FROM \"test_model\" ORDER BY \"a\" DESC LIMIT 5) \
             AS \"__m\" LEFT JOIN \"test_model\" AS \"__r0\" \
             ON \"__r0\".\"id\" = \"__m\".\"parent_id\" ORDER BY \"a\" DESC"
        );
    }

    #[test]
    fn order_by_quotes_identifier() {
        let (sql, _) = Filter::<T>::new(Dialect::Postgres)
            .order_by("x\" ; DROP TABLE t; --")
            .select_sql();
        assert!(sql.contains("\"x\"\" ; DROP TABLE t; --\" ASC"), "{sql}");
    }

    #[test]
    fn dialect_quirks() {
        let xor = |d| {
            Filter::<T>::new(d)
                .eq("a", 1i32)
                .xor()
                .eq("b", 2i32)
                .build_where()
                .0
        };
        assert!(xor(Dialect::Postgres).contains("XOR"));
        assert!(xor(Dialect::Mysql).contains("XOR"));
        assert!(xor(Dialect::Sqlite).contains("OR \"b\"") && !xor(Dialect::Sqlite).contains("XOR"));

        let sum = |d| Filter::<T>::new(d).aggregate_sql("SUM", "a").0;
        assert_eq!(
            sum(Dialect::Postgres),
            "SELECT SUM(\"a\")::float8 FROM \"test_model\""
        );
        assert_eq!(
            sum(Dialect::Sqlite),
            "SELECT CAST(SUM(\"a\") AS REAL) FROM \"test_model\""
        );
        assert_eq!(
            sum(Dialect::Mysql),
            "SELECT CAST(SUM(`a`) AS DOUBLE) FROM `test_model`"
        );

        let ilike = |d| {
            Filter::<T>::new(d)
                .ilike("a", "x%".to_string())
                .build_where()
                .0
        };
        assert_eq!(ilike(Dialect::Postgres), "WHERE \"a\" ILIKE $1");
        assert_eq!(ilike(Dialect::Sqlite), "WHERE \"a\" LIKE ?");
        assert_eq!(ilike(Dialect::Mysql), "WHERE LOWER(`a`) LIKE LOWER(?)");

        let empty = Filter::<T>::new(Dialect::Postgres)
            .in_values("a", Vec::new())
            .build_where()
            .0;
        assert_eq!(empty, "WHERE (1 = 0)");
    }
}
