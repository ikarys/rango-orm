use crate::error::db_err;
use crate::ops::bind_sql_values;
use rango_core::Result;
use rango_core::{quote_ident_mysql as quote_ident, FromRow, Model, ModelValues, SqlValue};
use sqlx::MySqlPool;
// ─── Helpers ─────────────────────────────────────────────────────────────────
use sqlx::mysql::MySqlRow;

mod exec;
mod filter;

pub const DEFAULT_QUERY_LIMIT: i64 = 1000;

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
    values: Option<Vec<SqlValue>>,
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
}

#[derive(Debug, Clone)]
struct ConditionGroup {
    node: ConditionNode,
    connector: Connector,
}

pub struct QueryBuilder<M> {
    pool: MySqlPool,
    conditions: Vec<ConditionGroup>,
    order_by: Vec<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    explicit_limit: bool,
    next_connector: Connector,
    _phantom: std::marker::PhantomData<M>,
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
                Connector::And => format!("AND {}", expr),
                Connector::Or => format!("OR {}", expr),
                Connector::AndNot => format!("AND NOT {}", expr),
                Connector::OrNot => format!("OR NOT {}", expr),
                Connector::Xor => format!("XOR {}", expr),
            };
            parts.push(conn);
        }
    }
    parts.join(" ")
}

fn build_condition(c: &Condition, binds: &mut Vec<SqlValue>) -> String {
    let col = quote_ident(&c.column);
    match &c.op {
        Op::IsNull => format!("{} IS NULL", col),
        Op::IsNotNull => format!("{} IS NOT NULL", col),
        Op::In => {
            let vals = c.values.as_ref().unwrap();
            let placeholders: Vec<String> = vals.iter().map(|_| "?".to_string()).collect();
            binds.extend(vals.clone());
            format!("{} IN ({})", col, placeholders.join(", "))
        }
        op => {
            let op_str = match op {
                Op::Eq => "=",
                Op::Ne => "!=",
                Op::Gt => ">",
                Op::Gte => ">=",
                Op::Lt => "<",
                Op::Lte => "<=",
                // Case-sensitive like Postgres, whatever the column collation
                Op::Like => {
                    binds.push(c.value.clone().unwrap());
                    return format!("{} COLLATE utf8mb4_bin LIKE ?", col);
                }
                Op::ILike => {
                    binds.push(c.value.clone().unwrap());
                    return format!("LOWER({}) LIKE LOWER(?)", col);
                }
                _ => unreachable!(),
            };
            binds.push(c.value.clone().unwrap());
            format!("{} {} ?", col, op_str)
        }
    }
}

async fn bind_and_fetch_all(
    pool: &MySqlPool,
    sql: &str,
    binds: Vec<SqlValue>,
) -> Result<Vec<MySqlRow>> {
    let q = sqlx::query(sql);
    let q = bind_sql_values(q, binds);
    q.fetch_all(pool).await.map_err(db_err)
}

async fn bind_and_fetch_one(pool: &MySqlPool, sql: &str, binds: Vec<SqlValue>) -> Result<MySqlRow> {
    let q = sqlx::query(sql);
    let q = bind_sql_values(q, binds);
    q.fetch_one(pool).await.map_err(db_err)
}

// ─── RangoFilterExt trait ─────────────────────────────────────────────────────
pub trait RangoFilterExt: Model + ModelValues + FromRow + Sized {
    fn filter(pool: &MySqlPool) -> QueryBuilder<Self> {
        QueryBuilder::new(pool)
    }
}

impl<M: Model + ModelValues + FromRow> RangoFilterExt for M {}
