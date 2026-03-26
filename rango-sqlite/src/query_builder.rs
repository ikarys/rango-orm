use rango_core::{FromRow, Model, ModelValues, SqlValue};
use sqlx::SqlitePool;
use anyhow::Result;

use crate::row::SqliteRangoRow;
use crate::ops::bind_sql_values;

pub const DEFAULT_QUERY_LIMIT: i64 = 1000;

#[derive(Debug, Clone)]
enum Op { Eq, Ne, Gt, Gte, Lt, Lte, Like, ILike, In, IsNull, IsNotNull }

#[derive(Debug, Clone)]
struct Condition {
    column: String,
    op: Op,
    value: Option<SqlValue>,
    values: Option<Vec<SqlValue>>,
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

pub struct QueryBuilder<M> {
    pool: SqlitePool,
    conditions: Vec<ConditionGroup>,
    order_by: Vec<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    explicit_limit: bool,
    next_connector: Connector,
    _phantom: std::marker::PhantomData<M>,
}

impl<M> QueryBuilder<M>
where
    M: Model + ModelValues + FromRow,
{
    pub fn new(pool: &SqlitePool) -> Self {
        Self {
            pool: pool.clone(),
            conditions: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
            explicit_limit: false,
            next_connector: Connector::And,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn eq(self, col: &str, val: impl Into<SqlValue>) -> Self { self.add(col, Op::Eq, Some(val.into()), None) }
    pub fn ne(self, col: &str, val: impl Into<SqlValue>) -> Self { self.add(col, Op::Ne, Some(val.into()), None) }
    pub fn gt(self, col: &str, val: impl Into<SqlValue>) -> Self { self.add(col, Op::Gt, Some(val.into()), None) }
    pub fn gte(self, col: &str, val: impl Into<SqlValue>) -> Self { self.add(col, Op::Gte, Some(val.into()), None) }
    pub fn lt(self, col: &str, val: impl Into<SqlValue>) -> Self { self.add(col, Op::Lt, Some(val.into()), None) }
    pub fn lte(self, col: &str, val: impl Into<SqlValue>) -> Self { self.add(col, Op::Lte, Some(val.into()), None) }
    pub fn like(self, col: &str, pattern: impl Into<String>) -> Self { self.add(col, Op::Like, Some(SqlValue::Text(pattern.into())), None) }
    pub fn ilike(self, col: &str, pattern: impl Into<String>) -> Self { self.add(col, Op::ILike, Some(SqlValue::Text(pattern.into())), None) }
    pub fn in_values(self, col: &str, vals: Vec<SqlValue>) -> Self { self.add(col, Op::In, None, Some(vals)) }
    pub fn is_null(self, col: &str) -> Self { self.add(col, Op::IsNull, None, None) }
    pub fn is_not_null(self, col: &str) -> Self { self.add(col, Op::IsNotNull, None, None) }

    pub fn or(mut self) -> Self { self.next_connector = Connector::Or; self }
    #[allow(clippy::should_implement_trait)]
    pub fn not(mut self) -> Self { self.next_connector = Connector::AndNot; self }
    pub fn or_not(mut self) -> Self { self.next_connector = Connector::OrNot; self }
    pub fn xor(mut self) -> Self { self.next_connector = Connector::Xor; self }

    pub fn group<F>(mut self, f: F) -> Self
    where F: FnOnce(QueryBuilder<M>) -> QueryBuilder<M>,
    {
        let inner = QueryBuilder::<M>::new(&self.pool);
        let inner = f(inner);
        let connector = std::mem::replace(&mut self.next_connector, Connector::And);
        self.conditions.push(ConditionGroup { node: ConditionNode::Group(inner.conditions), connector });
        self
    }

    pub fn order_by(mut self, col: &str) -> Self {
        let order = if let Some(col) = col.strip_prefix('-') {
            format!("\"{}\" DESC", col)
        } else {
            format!("\"{}\" ASC", col)
        };
        self.order_by.push(order);
        self
    }

    pub fn limit(mut self, n: i64) -> Self { self.limit = Some(n); self.explicit_limit = true; self }
    pub fn offset(mut self, n: i64) -> Self { self.offset = Some(n); self }
    pub fn unlimited(mut self) -> Self { self.explicit_limit = true; self }

    fn add(mut self, col: &str, op: Op, value: Option<SqlValue>, values: Option<Vec<SqlValue>>) -> Self {
        let connector = std::mem::replace(&mut self.next_connector, Connector::And);
        self.conditions.push(ConditionGroup {
            node: ConditionNode::Single(Condition { column: col.to_string(), op, value, values }),
            connector,
        });
        self
    }

    fn build_where(&self) -> (String, Vec<SqlValue>) {
        if self.conditions.is_empty() { return (String::new(), Vec::new()); }
        let mut binds = Vec::new();
        let expr = build_conditions(&self.conditions, &mut binds);
        (format!("WHERE {}", expr), binds)
    }

    pub fn explain(&self) -> String {
        let table = M::table_name();
        let (where_clause, _) = self.build_where();
        let mut sql = format!("SELECT * FROM \"{}\"", table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        if !self.order_by.is_empty() { sql.push_str(&format!(" ORDER BY {}", self.order_by.join(", "))); }
        if let Some(l) = self.limit { sql.push_str(&format!(" LIMIT {}", l)); }
        else if !self.explicit_limit { sql.push_str(&format!(" LIMIT {}", DEFAULT_QUERY_LIMIT)); }
        if let Some(o) = self.offset { sql.push_str(&format!(" OFFSET {}", o)); }
        sql
    }

    pub async fn all(self) -> Result<Vec<M>> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT * FROM \"{}\"", table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        if !self.order_by.is_empty() { sql.push_str(&format!(" ORDER BY {}", self.order_by.join(", "))); }
        if !self.explicit_limit { sql.push_str(&format!(" LIMIT {}", DEFAULT_QUERY_LIMIT)); }
        else if let Some(l) = self.limit { sql.push_str(&format!(" LIMIT {}", l)); }
        if let Some(o) = self.offset { sql.push_str(&format!(" OFFSET {}", o)); }
        let rows = bind_and_fetch_all(&self.pool, &sql, binds).await?;
        rows.into_iter()
            .map(|r| M::from_row(&SqliteRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e)))
            .collect()
    }

    pub async fn one(self) -> Result<Option<M>> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT * FROM \"{}\"", table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        sql.push_str(" LIMIT 1");
        let rows = bind_and_fetch_all(&self.pool, &sql, binds).await?;
        match rows.into_iter().next() {
            Some(r) => Ok(Some(M::from_row(&SqliteRangoRow(r)).map_err(|e| anyhow::anyhow!("{}", e))?)),
            None => Ok(None),
        }
    }

    pub async fn count(self) -> Result<i64> {
        use sqlx::Row;
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT COUNT(*) FROM \"{}\"", table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<i64, _>(0).unwrap_or(0))
    }

    pub async fn exists(self) -> Result<bool> { Ok(self.count().await? > 0) }

    pub async fn sum(self, col: &str) -> Result<Option<f64>> {
        use sqlx::Row;
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT CAST(SUM(\"{}\") AS REAL) FROM \"{}\"", col, table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<Option<f64>, _>(0).unwrap_or(None))
    }

    pub async fn avg(self, col: &str) -> Result<Option<f64>> {
        use sqlx::Row;
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT AVG(\"{}\") FROM \"{}\"", col, table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<Option<f64>, _>(0).unwrap_or(None))
    }

    pub async fn min<T>(self, col: &str) -> Result<Option<T>>
    where T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
    {
        use sqlx::Row;
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT MIN(\"{}\") FROM \"{}\"", col, table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<Option<T>, _>(0).unwrap_or(None))
    }

    pub async fn max<T>(self, col: &str) -> Result<Option<T>>
    where T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
    {
        use sqlx::Row;
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!("SELECT MAX(\"{}\") FROM \"{}\"", col, table);
        if !where_clause.is_empty() { sql.push(' '); sql.push_str(&where_clause); }
        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<Option<T>, _>(0).unwrap_or(None))
    }
}

// ─── SQL builders ─────────────────────────────────────────────────────────────

fn build_conditions(groups: &[ConditionGroup], binds: &mut Vec<SqlValue>) -> String {
    let mut parts = Vec::new();
    for (i, cg) in groups.iter().enumerate() {
        let expr = match &cg.node {
            ConditionNode::Single(c) => build_condition(c, binds),
            ConditionNode::Group(inner) => format!("({})", build_conditions(inner, binds)),
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
                Connector::Xor    => format!("OR {}", expr), // SQLite has no XOR
            };
            parts.push(conn);
        }
    }
    parts.join(" ")
}

fn build_condition(c: &Condition, binds: &mut Vec<SqlValue>) -> String {
    let col = format!("\"{}\"", c.column);
    match &c.op {
        Op::IsNull    => format!("{} IS NULL", col),
        Op::IsNotNull => format!("{} IS NOT NULL", col),
        Op::In => {
            let vals = c.values.as_ref().unwrap();
            let placeholders: Vec<String> = vals.iter().map(|_| "?".to_string()).collect();
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
                Op::Like | Op::ILike => "LIKE", // SQLite LIKE is case-insensitive by default
                _ => unreachable!(),
            };
            binds.push(c.value.clone().unwrap());
            format!("{} {} ?", col, op_str)
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

use sqlx::sqlite::SqliteRow;

async fn bind_and_fetch_all(pool: &SqlitePool, sql: &str, binds: Vec<SqlValue>) -> Result<Vec<SqliteRow>> {
    let q = sqlx::query(sql);
    let q = bind_sql_values(q, binds);
    q.fetch_all(pool).await.map_err(|e| anyhow::anyhow!("{}", e))
}

async fn bind_and_fetch_one(pool: &SqlitePool, sql: &str, binds: Vec<SqlValue>) -> Result<SqliteRow> {
    let q = sqlx::query(sql);
    let q = bind_sql_values(q, binds);
    q.fetch_one(pool).await.map_err(|e| anyhow::anyhow!("{}", e))
}

// ─── RangoFilterExt trait ─────────────────────────────────────────────────────

pub trait RangoFilterExt: Model + ModelValues + FromRow + Sized {
    fn filter(pool: &SqlitePool) -> QueryBuilder<Self> {
        QueryBuilder::new(pool)
    }
}

impl<M: Model + ModelValues + FromRow> RangoFilterExt for M {}
