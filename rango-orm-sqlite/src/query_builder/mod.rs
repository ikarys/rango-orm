use crate::error::db_err;
use crate::ops::bind_sql_values;
use rango_orm_core::filter::{Dialect, Filter};
use rango_orm_core::Result;
use rango_orm_core::SqlValue;
use rango_orm_core::{FromRow, Model, ModelValues};
use sqlx::sqlite::SqliteRow;
use sqlx::Executor;

mod conn;
pub use conn::Conn;
mod exec;
pub use exec::SelectAs;

pub use rango_orm_core::DEFAULT_QUERY_LIMIT;

/// Chainable, lazy query builder. SQL is built and executed only at `.all()` / `.one()`.
///
/// The filters and the SQL they render to live in the DB-free `Filter`; this type adds
/// the connection and the sqlx execution.
pub struct QueryBuilder<'a, M> {
    conn: Conn<'a>,
    f: Filter<M>,
}

impl<'a, M> QueryBuilder<'a, M>
where
    M: Model + ModelValues + FromRow,
{
    pub fn new(conn: impl Into<Conn<'a>>) -> Self {
        Self {
            conn: conn.into(),
            f: Filter::new(Dialect::Sqlite),
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

    /// Add a raw WHERE clause, for backend-specific extensions only.
    #[allow(dead_code)]
    pub(crate) fn raw_where(self, sql: String, binds: Vec<SqlValue>) -> Self {
        self.map(|f| f.raw_where(sql, binds))
    }

    /// Remember the first unknown column of model `T`; reported when the query runs.
    #[allow(dead_code)]
    pub(crate) fn check_col<T: Model>(&mut self, name: &str) {
        self.f.check_col::<T>(name);
    }

    /// Warn when the implicit row cap may have hidden rows.
    fn warn_if_capped(&self, rows: usize) {
        if self.f.is_capped(rows) {
            tracing::warn!(
                table = M::table_name(),
                cap = DEFAULT_QUERY_LIMIT,
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
    /// `&pool`, or `&mut **tx` inside `atomic`.
    fn filter<'a>(conn: impl Into<Conn<'a>>) -> QueryBuilder<'a, Self> {
        QueryBuilder::new(conn)
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
    async fn subqueries_keep_bind_order() {
        let pool = SqlitePool::connect_lazy("sqlite::memory:").unwrap();
        let (sql, binds) = QueryBuilder::<TestModel>::new(&pool)
            .eq("a", 1i32)
            .or_not()
            .where_exists::<TestModel>("parent_id", |q| q.eq("b", 2i32))
            .in_subquery::<TestModel>("c", "id", |q| q)
            .eq("d", 3i32)
            .f
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
