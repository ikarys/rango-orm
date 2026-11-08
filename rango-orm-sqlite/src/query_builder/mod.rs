use crate::error::db_err;
use crate::ops::bind_sql_values;
use rango_orm_core::related::{CollectVecFn, FromRowFn};
use rango_orm_core::Result;
use rango_orm_core::{quote_ident, FromRow, Model, ModelValues, SqlValue};
use sqlx::sqlite::SqliteRow;
use sqlx::{Executor, SqlitePool};
use std::any::TypeId;

mod exec;
pub use exec::SelectAs;
mod filter;
mod relations;

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
    Subquery(Box<Subquery>),
}

/// `EXISTS (...)` / `col IN (...)` built from another `QueryBuilder`'s filters.
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

/// Spec for a `.select_related(fk_col)` call.
/// The FK is on the main model's table: `m.fk_col = r.pk`, loaded with a LEFT JOIN.
struct SelectRelatedSpec {
    /// Column on M's table containing the FK to R.
    fk_col: String,
    /// `TypeId::of::<R>()` — used as the `WithRelated` cache key.
    type_id: TypeId,
    /// R's table name.
    table_name: &'static str,
    /// R's primary key column name.
    pk_col: &'static str,
    /// R's columns, selected as `"r{i}"."col" AS "__r{i}_col"`.
    columns: Vec<String>,
    /// Type-erased `R::from_row`.
    from_row: FromRowFn,
}

/// Spec for a `.prefetch_related(..)` call: a reverse FK or a M2M field.
struct PrefetchSpec {
    /// SQL up to and including the key expression, without the `= ANY(..)` / `IN (..)` part.
    select: String,
    /// Name of the result column holding M's primary key value in each fetched row.
    key_col: String,
    /// `TypeId::of::<Vec<R>>()` — used as the `WithRelated` cache key.
    vec_type_id: TypeId,
    /// Type-erased `R::from_row`.
    from_row: FromRowFn,
    /// Collects `Vec<Box<dyn Any>>` into a `Box<dyn Any>` containing `Vec<R>`.
    collect_vec: CollectVecFn,
}

pub struct QueryBuilder<M> {
    pool: SqlitePool,
    conditions: Vec<ConditionGroup>,
    order_by: Vec<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    explicit_limit: bool,
    next_connector: Connector,
    /// Raw WHERE clauses injected by SqliteQueryExt. Each entry is (sql_fragment, binds).
    pub(crate) raw_conditions: Vec<(String, Vec<SqlValue>)>,
    select_related: Vec<SelectRelatedSpec>,
    prefetch_related: Vec<PrefetchSpec>,
    /// First unknown column met while building; returned as `Error::Invalid` on execution.
    invalid: Option<String>,
    _phantom: std::marker::PhantomData<M>,
}

// ─── SQL builders ─────────────────────────────────────────────────────────────
fn build_conditions(groups: &[ConditionGroup], binds: &mut Vec<SqlValue>) -> String {
    let mut parts = Vec::new();
    for (i, cg) in groups.iter().enumerate() {
        let expr = match &cg.node {
            ConditionNode::Single(c) => build_condition(c, binds),
            ConditionNode::Group(inner) => format!("({})", build_conditions(inner, binds)),
            ConditionNode::Subquery(sub) => {
                let body = build_where_body(&sub.conditions, &sub.raw_conditions, binds);
                match (body.is_empty(), sub.has_where) {
                    (true, _) => format!("{})", sub.head),
                    (false, true) => format!("{} AND ({}))", sub.head, body),
                    (false, false) => format!("{} WHERE {})", sub.head, body),
                }
            }
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
                Connector::Xor => format!("OR {}", expr), // SQLite has no XOR
            };
            parts.push(conn);
        }
    }
    parts.join(" ")
}

/// Filters joined with AND, without the `WHERE` keyword. Empty when there is no filter.
fn build_where_body(
    conditions: &[ConditionGroup],
    raw_conditions: &[(String, Vec<SqlValue>)],
    binds: &mut Vec<SqlValue>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !conditions.is_empty() {
        let expr = build_conditions(conditions, binds);
        // Parenthesize so an OR chain does not swallow the raw conditions after it.
        parts.push(if raw_conditions.is_empty() {
            expr
        } else {
            format!("({expr})")
        });
    }
    // Raw conditions already contain `?` placeholders; append their binds in order.
    for (sql, raw_binds) in raw_conditions {
        binds.extend(raw_binds.clone());
        parts.push(sql.clone());
    }
    parts.join(" AND ")
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
                Op::Like | Op::ILike => "LIKE", // SQLite LIKE is case-insensitive by default
                _ => unreachable!(),
            };
            binds.push(c.value.clone().unwrap());
            format!("{} {} ?", col, op_str)
        }
    }
}

async fn bind_and_fetch_all<'e, E>(
    executor: E,
    sql: &str,
    binds: Vec<SqlValue>,
) -> Result<Vec<SqliteRow>>
where
    E: Executor<'e, Database = sqlx::Sqlite>,
{
    let q = sqlx::query(sqlx::AssertSqlSafe(sql));
    let q = bind_sql_values(q, binds);
    q.fetch_all(executor).await.map_err(db_err)
}

async fn bind_and_fetch_one<'e, E>(
    executor: E,
    sql: &str,
    binds: Vec<SqlValue>,
) -> Result<SqliteRow>
where
    E: Executor<'e, Database = sqlx::Sqlite>,
{
    let q = sqlx::query(sqlx::AssertSqlSafe(sql));
    let q = bind_sql_values(q, binds);
    q.fetch_one(executor).await.map_err(db_err)
}

// ─── RangoFilterExt trait ─────────────────────────────────────────────────────
pub trait RangoFilterExt: Model + ModelValues + FromRow + Sized {
    fn filter(pool: &SqlitePool) -> QueryBuilder<Self> {
        QueryBuilder::new(pool)
    }
}

impl<M: Model + ModelValues + FromRow> RangoFilterExt for M {}

#[cfg(test)]
mod tests {
    use super::QueryBuilder;
    use rango_orm_core::{FromRow, Model, ModelValues, RangoRow, RowError, SqlValue, TableSchema};
    use sqlx::SqlitePool;

    #[derive(Debug)]
    struct TestModel;

    impl Model for TestModel {
        fn table_name() -> &'static str {
            "test_model"
        }

        fn schema() -> TableSchema {
            TableSchema::default()
        }
    }

    impl ModelValues for TestModel {
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

    impl FromRow for TestModel {
        fn from_row(_row: &dyn RangoRow) -> Result<Self, RowError> {
            Ok(Self)
        }
    }

    #[tokio::test]
    async fn subqueries_keep_bind_order() {
        let pool = SqlitePool::connect_lazy("sqlite::memory:").unwrap();
        let (sql, binds) = QueryBuilder::<TestModel>::new(&pool)
            .eq("a", 1i32)
            .or_not()
            .where_exists::<TestModel>("parent_id", |q| q.eq("b", 2i32))
            .in_subquery::<TestModel>("c", "id", |q| q)
            .eq("d", 3i32)
            .build_where();

        assert_eq!(
            sql,
            r#"WHERE "a" = ? OR NOT EXISTS (SELECT 1 FROM "test_model" AS "__sub" WHERE "__sub"."parent_id" = "test_model"."id" AND ("b" = ?)) AND "c" IN (SELECT "id" FROM "test_model") AND "d" = ?"#
        );
        assert_eq!(format!("{:?}", binds), "[Int(1), Int(2), Int(3)]");
    }

    #[tokio::test]
    async fn select_related_joins_after_paginating() {
        let pool = SqlitePool::connect_lazy("sqlite::memory:").unwrap();
        let sql = QueryBuilder::<TestModel>::new(&pool)
            .select_related::<TestModel>("parent_id")
            .order_by("-a")
            .limit(5)
            .explain();
        assert_eq!(
            sql,
            "SELECT \"__m\".* FROM (SELECT * FROM \"test_model\" ORDER BY \"a\" DESC LIMIT 5) \
             AS \"__m\" LEFT JOIN \"test_model\" AS \"__r0\" \
             ON \"__r0\".\"id\" = \"__m\".\"parent_id\" ORDER BY \"a\" DESC"
        );
    }
}
