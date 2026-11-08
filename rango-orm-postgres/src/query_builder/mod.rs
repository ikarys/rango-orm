use crate::error::db_err;
use rango_orm_core::Result;
use rango_orm_core::{SqlValue, quote_ident};
use sqlx::PgPool;
use std::any::TypeId;
// ─── Bind helpers ─────────────────────────────────────────────────────────────
use crate::ops::bind_sql_values;
use sqlx::postgres::PgRow;

mod exec;
pub use exec::SelectAs;
mod filter;
mod relations;

// ─── Type aliases ─────────────────────────────────────────────────────────────
use rango_orm_core::related::{CollectVecFn, FromRowFn};

// ─── Filter types ─────────────────────────────────────────────────────────────
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

// ─── Relation specs ───────────────────────────────────────────────────────────
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
    /// When false, `all()` applies `DEFAULT_QUERY_LIMIT` (1000 rows) automatically.
    pub explicit_limit: bool,
    next_connector: Connector,
    /// Introspection: `(fk_col, related_table, related_pk)` for each `.select_related()` call.
    pub select_related_specs: Vec<(String, String, String)>,
    /// Introspection: `(related_table, related_pk, fk_col)` for each `.prefetch_related()` call.
    pub prefetch_specs: Vec<(String, String, String)>,
    select_related: Vec<SelectRelatedSpec>,
    prefetch_related: Vec<PrefetchSpec>,
    /// Raw WHERE clauses injected by backend-specific extensions (e.g. PgQueryExt).
    /// Each entry is (sql_fragment, bind_values) where `$__` is replaced by `$N`.
    pub(crate) raw_conditions: Vec<(String, Vec<SqlValue>)>,
    /// First unknown column met while building; returned as `Error::Invalid` on execution.
    invalid: Option<String>,
    _phantom: std::marker::PhantomData<M>,
}

// ─── SQL builder helpers ──────────────────────────────────────────────────────
fn build_conditions(
    groups: &[ConditionGroup],
    binds: &mut Vec<SqlValue>,
    idx: &mut usize,
) -> String {
    let mut parts = Vec::new();
    for (i, cg) in groups.iter().enumerate() {
        let expr = match &cg.node {
            ConditionNode::Single(c) => build_condition(c, binds, idx),
            ConditionNode::Group(inner) => format!("({})", build_conditions(inner, binds, idx)),
            ConditionNode::Subquery(sub) => {
                let body = build_where_body(&sub.conditions, &sub.raw_conditions, binds, idx);
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
                Connector::Xor => format!("XOR {}", expr),
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
    idx: &mut usize,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !conditions.is_empty() {
        let expr = build_conditions(conditions, binds, idx);
        // Parenthesize so an OR chain does not swallow the raw conditions after it.
        parts.push(if raw_conditions.is_empty() {
            expr
        } else {
            format!("({expr})")
        });
    }
    // Raw conditions: replace each $__ with the current $N.
    for (sql, raw_binds) in raw_conditions {
        let mut fragment = sql.clone();
        for bind in raw_binds {
            fragment = fragment.replacen("$__", &format!("${}", idx), 1);
            binds.push(bind.clone());
            *idx += 1;
        }
        parts.push(fragment);
    }
    parts.join(" AND ")
}

fn build_condition(c: &Condition, binds: &mut Vec<SqlValue>, idx: &mut usize) -> String {
    let col = quote_ident(&c.column);
    match &c.op {
        Op::IsNull => format!("{} IS NULL", col),
        Op::IsNotNull => format!("{} IS NOT NULL", col),
        Op::In => {
            let vals = c.values.as_ref().unwrap();
            let placeholders: Vec<String> = vals
                .iter()
                .map(|_| {
                    let p = format!("${}", idx);
                    *idx += 1;
                    p
                })
                .collect();
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
                Op::Like => "LIKE",
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

async fn bind_and_fetch_all<'e, E>(
    executor: E,
    sql: &str,
    binds: Vec<SqlValue>,
) -> Result<Vec<PgRow>>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let q = sqlx::query(sqlx::AssertSqlSafe(sql));
    let q = bind_sql_values(q, binds);
    q.fetch_all(executor).await.map_err(db_err)
}

/// Run `sql` with the keys bound as one Postgres array (`$1`).
/// Keys come from `sql_value_key`, so they are UUIDs, integers or text, all of one type.
async fn fetch_by_keys<'e, E>(executor: E, sql: &str, keys: Vec<SqlValue>) -> Result<Vec<PgRow>>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    macro_rules! collect {
        ($variant:ident) => {
            keys.into_iter()
                .filter_map(|k| match k {
                    SqlValue::$variant(v) => Some(v),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
    }
    let q = sqlx::query(sqlx::AssertSqlSafe(sql));
    let q = match keys.first() {
        Some(SqlValue::Uuid(_)) => q.bind(collect!(Uuid)),
        Some(SqlValue::BigInt(_)) => q.bind(collect!(BigInt)),
        Some(SqlValue::Int(_)) => q.bind(collect!(Int)),
        Some(SqlValue::SmallInt(_)) => q.bind(collect!(SmallInt)),
        _ => q.bind(collect!(Text)),
    };
    q.fetch_all(executor).await.map_err(db_err)
}

async fn bind_and_fetch_one(pool: &PgPool, sql: &str, binds: Vec<SqlValue>) -> Result<PgRow> {
    let q = sqlx::query(sqlx::AssertSqlSafe(sql));
    let q = bind_sql_values(q, binds);
    q.fetch_one(pool).await.map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::QueryBuilder;
    use rango_orm_core::{FromRow, Model, ModelValues, RangoRow, RowError, SqlValue, TableSchema};
    use sqlx::PgPool;

    #[derive(Debug)]
    struct TestModel;

    impl rango_orm_core::ModelHooks for TestModel {}

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
    async fn subqueries_number_their_placeholders_in_order() {
        let pool = PgPool::connect_lazy("postgres://localhost/x").unwrap();
        let sql = QueryBuilder::<TestModel>::new(&pool)
            .eq("a", 1i32)
            .or_not()
            .where_exists::<TestModel>("parent_id", |q| q.eq("b", 2i32))
            .in_subquery::<TestModel>("c", "id", |q| q)
            .eq("d", 3i32)
            .explain();
        assert_eq!(
            sql,
            "SELECT * FROM \"test_model\" WHERE \"a\" = $1 \
             OR NOT EXISTS (SELECT 1 FROM \"test_model\" AS \"__sub\" \
             WHERE \"__sub\".\"parent_id\" = \"test_model\".\"id\" AND (\"b\" = $2)) \
             AND \"c\" IN (SELECT \"id\" FROM \"test_model\") AND \"d\" = $3 LIMIT 1000"
        );
    }

    #[tokio::test]
    async fn select_related_joins_after_paginating() {
        let pool = PgPool::connect_lazy("postgres://localhost/x").unwrap();
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

    #[tokio::test]
    async fn order_by_quotes_identifier() {
        let pool = PgPool::connect_lazy("postgres://localhost/x").unwrap();
        let sql = QueryBuilder::<TestModel>::new(&pool)
            .order_by("x\" ; DROP TABLE t; --")
            .explain();
        assert!(
            sql.contains("\"x\"\" ; DROP TABLE t; --\" ASC"),
            "sql: {sql}"
        );
    }
}
