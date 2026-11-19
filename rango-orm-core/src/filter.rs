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

/// Comparison used by `having(alias, cmp, value)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
}

impl Cmp {
    fn sql(self) -> &'static str {
        match self {
            Cmp::Eq => "=",
            Cmp::Ne => "!=",
            Cmp::Gt => ">",
            Cmp::Gte => ">=",
            Cmp::Lt => "<",
            Cmp::Lte => "<=",
        }
    }
}

/// Aggregate function of an `annotate_*` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AggFn {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl AggFn {
    fn name(self) -> &'static str {
        match self {
            AggFn::Count => "COUNT",
            AggFn::Sum => "SUM",
            AggFn::Avg => "AVG",
            AggFn::Min => "MIN",
            AggFn::Max => "MAX",
        }
    }
}

/// One `expr AS alias` of the aggregate query.
#[derive(Debug, Clone)]
struct Annotation {
    alias: String,
    func: AggFn,
    /// `None` for `COUNT(*)`.
    column: Option<String>,
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
    /// `SET col = value` pairs for `update()`, in call order.
    sets: Vec<(String, SqlValue)>,
    /// `GROUP BY` columns of `aggregate()`, in call order.
    group_by: Vec<String>,
    /// `annotate_*` aggregates of `aggregate()`, in call order.
    annotations: Vec<Annotation>,
    /// `HAVING` conditions on annotation aliases, joined with AND.
    having: Vec<(String, Cmp, SqlValue)>,
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
            sets: Vec::new(),
            group_by: Vec::new(),
            annotations: Vec::new(),
            having: Vec::new(),
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

    /// `SET col = value` for `update_sql`; typed like `eq`, string names checked at run time.
    pub fn set<V>(mut self, col: impl FilterColumn<M, V>, val: V) -> Self {
        self.check_col::<M>(col.column_name());
        let val = col.bind(val);
        self.sets.push((col.column_name().to_string(), val));
        self
    }

    // ── Aggregation (GROUP BY / HAVING) ───────────────────────────────────────
    /// `GROUP BY col` for `group_sql`; repeatable.
    pub fn group_by(mut self, col: impl ColumnRef<M>) -> Self {
        self.check_col::<M>(col.column_name());
        self.group_by.push(col.column_name().to_string());
        self
    }

    fn annotate(mut self, alias: &str, func: AggFn, column: Option<&str>) -> Self {
        if let Some(c) = column {
            self.check_col::<M>(c);
        }
        self.annotations.push(Annotation {
            alias: alias.to_string(),
            func,
            column: column.map(str::to_string),
        });
        self
    }

    /// `COUNT(*) AS alias`.
    pub fn annotate_count(self, alias: &str) -> Self {
        self.annotate(alias, AggFn::Count, None)
    }

    /// `SUM(col) AS alias` (a float).
    pub fn annotate_sum(self, alias: &str, col: impl ColumnRef<M>) -> Self {
        self.annotate(alias, AggFn::Sum, Some(col.column_name()))
    }

    /// `AVG(col) AS alias` (a float).
    pub fn annotate_avg(self, alias: &str, col: impl ColumnRef<M>) -> Self {
        self.annotate(alias, AggFn::Avg, Some(col.column_name()))
    }

    /// `MIN(col) AS alias` (keeps the column type).
    pub fn annotate_min(self, alias: &str, col: impl ColumnRef<M>) -> Self {
        self.annotate(alias, AggFn::Min, Some(col.column_name()))
    }

    /// `MAX(col) AS alias` (keeps the column type).
    pub fn annotate_max(self, alias: &str, col: impl ColumnRef<M>) -> Self {
        self.annotate(alias, AggFn::Max, Some(col.column_name()))
    }

    /// `HAVING <aggregate of alias> <cmp> value`; several calls are joined with AND.
    /// The alias must come from an `annotate_*` call (checked when the SQL is built).
    pub fn having(mut self, alias: &str, cmp: Cmp, value: impl Into<SqlValue>) -> Self {
        self.having.push((alias.to_string(), cmp, value.into()));
        self
    }

    fn is_alias(&self, name: &str) -> bool {
        self.annotations.iter().any(|a| a.alias == name)
    }

    fn annotation_expr(&self, a: &Annotation) -> String {
        match &a.column {
            None => "COUNT(*)".to_string(),
            Some(c) => self.dialect.aggregate(a.func.name(), &self.q(c)),
        }
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
        if !self.is_alias(name) {
            self.check_col::<M>(name);
        }
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
        let clause = self.where_clause_from(&mut binds, &mut 1);
        (clause, binds)
    }

    /// `WHERE ...` continuing an existing placeholder numbering (`idx`) and bind list.
    fn where_clause_from(&self, binds: &mut Vec<SqlValue>, idx: &mut usize) -> String {
        let body = self.where_body(&self.conditions, &self.raw_conditions, binds, idx);
        if body.is_empty() {
            body
        } else {
            format!("WHERE {body}")
        }
    }

    /// `update()` / `delete()` take a filter only: no ordering, paging or relations.
    fn ensure_write_shape(&self) -> Result<()> {
        self.ensure_valid()?;
        if !self.order_by.is_empty()
            || self.limit.is_some()
            || self.offset.is_some()
            || !self.select_related.is_empty()
            || !self.prefetch_related.is_empty()
        {
            return Err(Error::Invalid(
                "update() / delete() do not accept order_by, limit, offset, select_related or prefetch_related".into(),
            ));
        }
        Ok(())
    }

    /// `UPDATE t SET .. WHERE ..` from the `set()` pairs; set binds first, then the filter's.
    pub fn update_sql(&self) -> Result<(String, Vec<SqlValue>)> {
        self.ensure_write_shape()?;
        if self.sets.is_empty() {
            return Err(Error::Invalid(
                "update() needs at least one set(column, value)".into(),
            ));
        }
        let mut binds = Vec::with_capacity(self.sets.len());
        let mut idx = 1usize;
        let assignments: Vec<String> = self
            .sets
            .iter()
            .map(|(col, val)| {
                binds.push(val.clone());
                format!("{} = {}", self.q(col), self.dialect.placeholder(&mut idx))
            })
            .collect();
        let mut sql = format!(
            "UPDATE {} SET {}",
            self.q(M::table_name()),
            assignments.join(", ")
        );
        let where_clause = self.where_clause_from(&mut binds, &mut idx);
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        Ok((sql, binds))
    }

    /// `DELETE FROM t WHERE ..`; no WHERE when the builder has no filter (Django semantics).
    pub fn delete_sql(&self) -> Result<(String, Vec<SqlValue>)> {
        self.ensure_write_shape()?;
        let mut binds = Vec::new();
        let mut sql = format!("DELETE FROM {}", self.q(M::table_name()));
        let where_clause = self.where_clause_from(&mut binds, &mut 1);
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        Ok((sql, binds))
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
        let order_by = self.order_by_sql();
        sql.push_str(&order_by);
        sql.push_str(&self.limit_offset_sql());
        (sql, order_by, binds)
    }

    /// `" ORDER BY ..."` or empty.
    fn order_by_sql(&self) -> String {
        let clauses = self.order_clauses();
        if clauses.is_empty() {
            String::new()
        } else {
            format!(" ORDER BY {}", clauses.join(", "))
        }
    }

    /// `" LIMIT n OFFSET m"`: the default row cap unless `.limit()` or `.unlimited()` was called.
    fn limit_offset_sql(&self) -> String {
        let mut sql = String::new();
        if !self.explicit_limit {
            sql.push_str(&format!(" LIMIT {DEFAULT_QUERY_LIMIT}"));
        } else if let Some(l) = self.limit {
            sql.push_str(&format!(" LIMIT {l}"));
        }
        if let Some(o) = self.offset {
            sql.push_str(&format!(" OFFSET {o}"));
        }
        sql
    }

    /// `SELECT <group cols>, <aggregates> FROM t WHERE .. GROUP BY .. HAVING .. ORDER BY .. LIMIT ..`
    /// for `aggregate()`. WHERE binds first, then HAVING binds.
    pub fn group_sql(&self) -> Result<(String, Vec<SqlValue>)> {
        self.ensure_valid()?;
        if self.group_by.is_empty() && self.annotations.is_empty() {
            return Err(Error::Invalid(
                "aggregate() needs group_by() or annotate_*()".into(),
            ));
        }
        if !self.select_related.is_empty() || !self.prefetch_related.is_empty() {
            return Err(Error::Invalid(
                "aggregate() does not accept select_related or prefetch_related".into(),
            ));
        }
        let mut select: Vec<String> = self.group_by.iter().map(|c| self.q(c)).collect();
        for a in &self.annotations {
            select.push(format!(
                "{} AS {}",
                self.annotation_expr(a),
                self.q(&a.alias)
            ));
        }
        let mut binds = Vec::new();
        let mut idx = 1usize;
        let mut sql = format!(
            "SELECT {} FROM {}",
            select.join(", "),
            self.q(M::table_name())
        );
        let where_clause = self.where_clause_from(&mut binds, &mut idx);
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        if !self.group_by.is_empty() {
            let cols: Vec<String> = self.group_by.iter().map(|c| self.q(c)).collect();
            sql.push_str(&format!(" GROUP BY {}", cols.join(", ")));
        }
        if !self.having.is_empty() {
            let mut parts = Vec::with_capacity(self.having.len());
            for (alias, cmp, value) in &self.having {
                let Some(a) = self.annotations.iter().find(|a| &a.alias == alias) else {
                    return Err(Error::Invalid(format!(
                        "having(): unknown annotation `{alias}`"
                    )));
                };
                binds.push(value.clone());
                parts.push(format!(
                    "{} {} {}",
                    self.annotation_expr(a),
                    cmp.sql(),
                    self.dialect.placeholder(&mut idx)
                ));
            }
            sql.push_str(&format!(" HAVING {}", parts.join(" AND ")));
        }
        sql.push_str(&self.order_by_sql());
        sql.push_str(&self.limit_offset_sql());
        Ok((sql, binds))
    }

    /// Result keys of `group_sql`, in column order: `(key, source column whose type the
    /// value keeps)`. Group columns keep their own type, `MIN` / `MAX` keep the column's,
    /// `COUNT` / `SUM` / `AVG` have none (integer / float).
    pub fn aggregate_keys(&self) -> Vec<(String, Option<String>)> {
        let mut keys: Vec<(String, Option<String>)> = self
            .group_by
            .iter()
            .map(|c| (c.clone(), Some(c.clone())))
            .collect();
        for a in &self.annotations {
            let source = match a.func {
                AggFn::Min | AggFn::Max => a.column.clone(),
                AggFn::Count | AggFn::Sum | AggFn::Avg => None,
            };
            keys.push((a.alias.clone(), source));
        }
        keys
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

            /// `SET col = value` for `.update()`. Typed like `eq`: `.set(User::ACTIVE, false)`.
            pub fn set<V>(self, col: impl $crate::FilterColumn<M, V>, val: V) -> Self {
                self.map(|f| f.set(col, val))
            }

            /// `GROUP BY col` for `.aggregate()`; repeatable. `.group_by(Order::COUNTRY)` or `"country"`.
            pub fn group_by(self, col: impl $crate::ColumnRef<M>) -> Self {
                self.map(|f| f.group_by(col))
            }

            /// `COUNT(*) AS alias` for `.aggregate()`; decodes as `SqlValue::BigInt`.
            pub fn annotate_count(self, alias: &str) -> Self {
                self.map(|f| f.annotate_count(alias))
            }

            /// `SUM(col) AS alias` for `.aggregate()`; decodes as `SqlValue::Double`.
            pub fn annotate_sum(self, alias: &str, col: impl $crate::ColumnRef<M>) -> Self {
                self.map(|f| f.annotate_sum(alias, col))
            }

            /// `AVG(col) AS alias` for `.aggregate()`; decodes as `SqlValue::Double`.
            pub fn annotate_avg(self, alias: &str, col: impl $crate::ColumnRef<M>) -> Self {
                self.map(|f| f.annotate_avg(alias, col))
            }

            /// `MIN(col) AS alias` for `.aggregate()`; keeps the column type.
            pub fn annotate_min(self, alias: &str, col: impl $crate::ColumnRef<M>) -> Self {
                self.map(|f| f.annotate_min(alias, col))
            }

            /// `MAX(col) AS alias` for `.aggregate()`; keeps the column type.
            pub fn annotate_max(self, alias: &str, col: impl $crate::ColumnRef<M>) -> Self {
                self.map(|f| f.annotate_max(alias, col))
            }

            /// `HAVING <aggregate of alias> <cmp> value` for `.aggregate()`; calls are joined with AND.
            /// `.having("total", Cmp::Gt, 100.0)` after `.annotate_sum("total", Order::AMOUNT)`.
            pub fn having(
                self,
                alias: &str,
                cmp: $crate::Cmp,
                value: impl Into<$crate::SqlValue>,
            ) -> Self {
                self.map(|f| f.having(alias, cmp, value))
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

    /// Model with declared columns: `update_sql` / `delete_sql` run `ensure_valid`.
    struct W;

    impl crate::ModelHooks for W {}

    impl Model for W {
        type Pk = uuid::Uuid;
        fn table_name() -> &'static str {
            "test_model"
        }
        fn schema() -> TableSchema {
            let col = |name: &str| crate::ColumnDef {
                name: name.to_string(),
                col_type: crate::ColumnType::Text,
                nullable: false,
                primary_key: false,
                unique: false,
                default: None,
                references: None,
                index: false,
            };
            TableSchema {
                columns: ["a", "age", "active", "note"]
                    .into_iter()
                    .map(col)
                    .collect(),
                ..TableSchema::default()
            }
        }
    }

    impl ModelValues for W {
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

    impl FromRow for W {
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
    fn update_sql_binds_sets_then_filters() {
        let (sql, binds) = Filter::<W>::new(Dialect::Postgres)
            .lt("age", 18i32)
            .set("active", false)
            .set("note", "minor")
            .update_sql()
            .unwrap();
        assert_eq!(
            sql,
            "UPDATE \"test_model\" SET \"active\" = $1, \"note\" = $2 WHERE \"age\" < $3"
        );
        assert_eq!(binds.len(), 3);
        assert!(matches!(binds[0], SqlValue::Bool(false)));

        let (sql, _) = Filter::<W>::new(Dialect::Sqlite)
            .set("active", false)
            .eq("a", 1i32)
            .update_sql()
            .unwrap();
        assert_eq!(
            sql,
            "UPDATE \"test_model\" SET \"active\" = ? WHERE \"a\" = ?"
        );
    }

    #[test]
    fn delete_sql_with_and_without_filter() {
        let (sql, binds) = Filter::<W>::new(Dialect::Postgres)
            .eq("a", 1i32)
            .delete_sql()
            .unwrap();
        assert_eq!(sql, "DELETE FROM \"test_model\" WHERE \"a\" = $1");
        assert_eq!(binds.len(), 1);

        let (sql, binds) = Filter::<W>::new(Dialect::Mysql).delete_sql().unwrap();
        assert_eq!(sql, "DELETE FROM `test_model`");
        assert!(binds.is_empty());
    }

    #[test]
    fn write_shape_is_a_filter_only() {
        assert!(matches!(
            Filter::<W>::new(Dialect::Postgres).update_sql(),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            Filter::<W>::new(Dialect::Postgres).limit(1).delete_sql(),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            Filter::<W>::new(Dialect::Postgres)
                .order_by("a")
                .set("a", 1i32)
                .update_sql(),
            Err(Error::Invalid(_))
        ));
        assert!(
            Filter::<W>::new(Dialect::Postgres)
                .unlimited()
                .delete_sql()
                .is_ok()
        );
    }

    #[test]
    fn group_sql_renders_group_by_having_and_order() {
        let f = Filter::<W>::new(Dialect::Postgres)
            .eq("active", true)
            .group_by("note")
            .annotate_count("n")
            .annotate_sum("total", "age")
            .having("total", Cmp::Gt, 10i32)
            .order_by("-n");
        let (sql, binds) = f.group_sql().unwrap();
        assert_eq!(
            sql,
            "SELECT \"note\", COUNT(*) AS \"n\", SUM(\"age\")::float8 AS \"total\" \
             FROM \"test_model\" WHERE \"active\" = $1 GROUP BY \"note\" \
             HAVING SUM(\"age\")::float8 > $2 ORDER BY \"n\" DESC LIMIT 1000"
        );
        assert_eq!(binds.len(), 2);
        assert_eq!(
            f.aggregate_keys(),
            vec![
                ("note".to_string(), Some("note".to_string())),
                ("n".to_string(), None),
                ("total".to_string(), None),
            ]
        );
    }

    #[test]
    fn group_sql_without_group_by_is_one_row() {
        let f = Filter::<W>::new(Dialect::Mysql)
            .annotate_count("n")
            .annotate_max("oldest", "age")
            .unlimited();
        let (sql, binds) = f.group_sql().unwrap();
        assert_eq!(
            sql,
            "SELECT COUNT(*) AS `n`, MAX(`age`) AS `oldest` FROM `test_model`"
        );
        assert!(binds.is_empty());
        assert_eq!(
            f.aggregate_keys()[1],
            ("oldest".to_string(), Some("age".to_string()))
        );
    }

    #[test]
    fn group_sql_rejects_bad_shapes() {
        assert!(matches!(
            Filter::<W>::new(Dialect::Postgres).group_sql(),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            Filter::<W>::new(Dialect::Postgres)
                .annotate_count("n")
                .having("nope", Cmp::Gt, 1i32)
                .group_sql(),
            Err(Error::Invalid(_))
        ));
        // An unknown real column is still caught, an alias in order_by is not.
        assert!(matches!(
            Filter::<W>::new(Dialect::Sqlite)
                .group_by("nope")
                .group_sql(),
            Err(Error::Invalid(_))
        ));
        assert!(
            Filter::<W>::new(Dialect::Sqlite)
                .annotate_count("n")
                .order_by("n")
                .group_sql()
                .is_ok()
        );
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
