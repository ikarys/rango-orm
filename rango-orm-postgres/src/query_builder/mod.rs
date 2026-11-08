use crate::error::db_err;
use rango_orm_core::filter::{Dialect, Filter};
use rango_orm_core::{FromRow, Model, ModelValues, Result, SqlValue};
// ─── Bind helpers ─────────────────────────────────────────────────────────────
use crate::ops::bind_sql_values;
use sqlx::postgres::PgRow;

mod conn;
pub use conn::Conn;
mod exec;
pub use exec::SelectAs;

// ─── QueryBuilder ─────────────────────────────────────────────────────────────
/// Chainable, lazy query builder. SQL is built and executed only at `.all()` / `.one()`.
///
/// The filters and the SQL they render to live in the DB-free `Filter`; this type adds
/// the connection and the sqlx execution.
pub struct QueryBuilder<'a, M> {
    conn: Conn<'a>,
    // TODO: cache backend trait (level 2 — Redis, Memcached, custom)
    f: Filter<M>,
}

impl<'a, M> QueryBuilder<'a, M>
where
    M: Model + ModelValues + FromRow,
{
    pub fn new(conn: impl Into<Conn<'a>>) -> Self {
        Self {
            conn: conn.into(),
            f: Filter::new(Dialect::Postgres),
        }
    }

    /// A sub-builder (`group`, `where_exists`, ...): filters only, never runs.
    fn detached(f: Filter<M>) -> Self {
        Self {
            conn: Conn::Detached,
            f,
        }
    }

    fn map(mut self, g: impl FnOnce(Filter<M>) -> Filter<M>) -> Self {
        self.f = g(self.f);
        self
    }

    /// Add a raw WHERE clause — for use by backend-specific extensions only.
    /// Use `$__` as a placeholder; it will be replaced by `$N` at build time.
    pub(crate) fn raw_where(self, sql: String, binds: Vec<SqlValue>) -> Self {
        self.map(|f| f.raw_where(sql, binds))
    }

    /// Remember the first unknown column of model `T`; reported when the query runs.
    pub(crate) fn check_col<T: Model>(&mut self, name: &str) {
        self.f.check_col::<T>(name);
    }

    /// Warn when the implicit row cap may have hidden rows.
    fn warn_if_capped(&self, rows: usize) {
        if self.f.is_capped(rows) {
            tracing::warn!(
                table = M::table_name(),
                cap = rango_orm_core::DEFAULT_QUERY_LIMIT,
                "query returned exactly the default row cap, results may be truncated; \
                 call .limit(n) to choose a cap or .unlimited() to fetch every row"
            );
        }
    }

    /// Returns the SQL that would be executed by `.all()`, for debugging.
    pub fn explain(&self) -> String {
        self.f.select_sql().0
    }
}

rango_orm_core::impl_filter_methods!();

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

async fn bind_and_fetch_one<'e, E>(executor: E, sql: &str, binds: Vec<SqlValue>) -> Result<PgRow>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let q = sqlx::query(sqlx::AssertSqlSafe(sql));
    let q = bind_sql_values(q, binds);
    q.fetch_one(executor).await.map_err(db_err)
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
        type Pk = uuid::Uuid;

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
