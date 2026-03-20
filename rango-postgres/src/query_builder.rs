use rango_core::{FromRow, Model, ModelValues, SqlValue};
use sqlx::PgPool;
use anyhow::Result;

use crate::row::PgRangoRow;

/// Comparison operators
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

/// A single filter condition
#[derive(Debug, Clone)]
struct Condition {
    column: String,
    op: Op,
    value: Option<SqlValue>,
    values: Option<Vec<SqlValue>>,  // for IN
}

/// Logical connector between conditions
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
    Group(Vec<ConditionGroup>),  // parenthesized group
}

#[derive(Debug, Clone)]
struct ConditionGroup {
    node: ConditionNode,
    connector: Connector,
}

/// The query builder — chainable, lazy (SQL built at execution time).
pub struct QueryBuilder<M> {
    pool: PgPool,
    conditions: Vec<ConditionGroup>,
    order_by: Vec<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    next_connector: Connector,
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
            next_connector: Connector::And,
            _phantom: std::marker::PhantomData,
        }
    }

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

    /// WHERE col LIKE value
    pub fn like(self, col: &str, pattern: impl Into<String>) -> Self {
        self.add(col, Op::Like, Some(SqlValue::Text(pattern.into())), None)
    }

    /// WHERE col ILIKE value (case-insensitive, PostgreSQL)
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

    /// ORDER BY col ASC, or -col for DESC
    pub fn order_by(mut self, col: &str) -> Self {
        let order = if col.starts_with('-') {
            format!("\"{}\" DESC", &col[1..])
        } else {
            format!("\"{}\" ASC", col)
        };
        self.order_by.push(order);
        self
    }

    pub fn limit(mut self, n: i64) -> Self {
        self.limit = Some(n);
        self
    }

    pub fn offset(mut self, n: i64) -> Self {
        self.offset = Some(n);
        self
    }

    fn add(mut self, col: &str, op: Op, value: Option<SqlValue>, values: Option<Vec<SqlValue>>) -> Self {
        let connector = std::mem::replace(&mut self.next_connector, Connector::And);
        self.conditions.push(ConditionGroup {
            node: ConditionNode::Single(Condition { column: col.to_string(), op, value, values }),
            connector,
        });
        self
    }

    /// Build the WHERE clause and collect bind values.
    fn build_where(&self) -> (String, Vec<SqlValue>) {
        if self.conditions.is_empty() {
            return (String::new(), Vec::new());
        }
        let mut binds = Vec::new();
        let mut idx = 1usize;
        let expr = build_conditions(&self.conditions, &mut binds, &mut idx);
        (format!("WHERE {}", expr), binds)
    }

    /// Execute and return all matching rows.
    pub async fn all(self) -> Result<Vec<M>> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();

        let mut sql = format!("SELECT * FROM \"{}\"", table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        if !self.order_by.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", self.order_by.join(", ")));
        }
        if let Some(l) = self.limit  { sql.push_str(&format!(" LIMIT {}", l)); }
        if let Some(o) = self.offset { sql.push_str(&format!(" OFFSET {}", o)); }

        let rows = bind_and_fetch_all(&self.pool, &sql, binds).await?;
        rows.into_iter()
            .map(|r| M::from_row(&PgRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e)))
            .collect()
    }

    /// Execute and return first matching row.
    pub async fn one(self) -> Result<Option<M>> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();

        let mut sql = format!("SELECT * FROM \"{}\"", table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        sql.push_str(" LIMIT 1");

        let rows = bind_and_fetch_all(&self.pool, &sql, binds).await?;
        match rows.into_iter().next() {
            Some(r) => Ok(Some(M::from_row(&PgRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e))?)),
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
}

// ─── SQL builder helpers ──────────────────────────────────────────────────────

fn build_conditions(groups: &[ConditionGroup], binds: &mut Vec<SqlValue>, idx: &mut usize) -> String {
    let mut parts = Vec::new();

    for (i, cg) in groups.iter().enumerate() {
        let expr = match &cg.node {
            ConditionNode::Single(c) => build_condition(c, binds, idx),
            ConditionNode::Group(inner) => {
                format!("({})", build_conditions(inner, binds, idx))
            }
        };

        if i == 0 {
            // First condition — check if it's negated
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
                let p = format!("${}", idx);
                *idx += 1;
                p
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
            let p = format!("${}", idx);
            *idx += 1;
            binds.push(c.value.clone().unwrap());
            format!("{} {} {}", col, op_str, p)
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

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


