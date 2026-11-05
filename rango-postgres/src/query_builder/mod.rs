use crate::error::db_err;
use rango_core::Result;
use rango_core::{RowError, SqlValue, quote_ident};
use sqlx::PgPool;
use std::any::{Any, TypeId};
// ─── Bind helpers ─────────────────────────────────────────────────────────────
use crate::ops::bind_sql_values;
use sqlx::postgres::PgRow;

mod exec;
mod filter;
mod relations;

// ─── Type aliases ─────────────────────────────────────────────────────────────
type FromRowFn = fn(&dyn rango_core::RangoRow) -> Result<Box<dyn Any + Send + Sync>, RowError>;

type CollectVecFn = fn(Vec<Box<dyn Any + Send + Sync>>) -> Box<dyn Any + Send + Sync>;

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
}

#[derive(Debug, Clone)]
struct ConditionGroup {
    node: ConditionNode,
    connector: Connector,
}

// ─── Relation specs ───────────────────────────────────────────────────────────
/// Spec for a `.select_related::<R>(fk_col)` call.
/// The FK is on the main model's table: `m.fk_col = r.pk`.
struct SelectRelatedSpec {
    /// Column on M's table containing the FK to R.
    fk_col: String,
    /// `TypeId::of::<R>()` — used as the `WithRelated` cache key.
    type_id: TypeId,
    /// R's table name.
    table_name: &'static str,
    /// R's primary key column name.
    pk_col: &'static str,
    /// Type-erased `R::from_row`.
    from_row: FromRowFn,
}

/// Spec for a `.prefetch_related::<R>(related_fk_col)` call.
/// The FK is on R's table: `r.related_fk_col = m.pk`.
struct PrefetchSpec {
    /// Column on R's table pointing back to M.
    related_fk_col: String,
    /// `TypeId::of::<Vec<R>>()` — used as the `WithRelated` cache key.
    vec_type_id: TypeId,
    /// R's table name.
    table_name: &'static str,
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
    /// When false, `all()` applies [`DEFAULT_QUERY_LIMIT`] automatically.
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

async fn bind_and_fetch_all(pool: &PgPool, sql: &str, binds: Vec<SqlValue>) -> Result<Vec<PgRow>> {
    let q = sqlx::query(sql);
    let q = bind_sql_values(q, binds);
    q.fetch_all(pool).await.map_err(db_err)
}

async fn bind_and_fetch_one(pool: &PgPool, sql: &str, binds: Vec<SqlValue>) -> Result<PgRow> {
    let q = sqlx::query(sql);
    let q = bind_sql_values(q, binds);
    q.fetch_one(pool).await.map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::QueryBuilder;
    use rango_core::{FromRow, Model, ModelValues, RangoRow, RowError, SqlValue, TableSchema};
    use sqlx::PgPool;

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
