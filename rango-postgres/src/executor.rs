use sqlx::{PgPool, Postgres, Transaction};
use sqlx::postgres::PgArguments;
use sqlx::query::Query;
use std::future::Future;
use std::pin::Pin;

/// Abstracts over PgPool and Transaction — allows all Rango ops to work in both contexts.
pub trait RangoExecutor: Send + Sync {
    fn execute_query<'e>(
        &'e mut self,
        query: Query<'e, Postgres, PgArguments>,
    ) -> Pin<Box<dyn Future<Output = Result<sqlx::postgres::PgQueryResult, sqlx::Error>> + Send + 'e>>;

    fn fetch_all_query<'e>(
        &'e mut self,
        query: Query<'e, Postgres, PgArguments>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<sqlx::postgres::PgRow>, sqlx::Error>> + Send + 'e>>;

    fn fetch_optional_query<'e>(
        &'e mut self,
        query: Query<'e, Postgres, PgArguments>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<sqlx::postgres::PgRow>, sqlx::Error>> + Send + 'e>>;
}

impl RangoExecutor for PgPool {
    fn execute_query<'e>(
        &'e mut self,
        query: Query<'e, Postgres, PgArguments>,
    ) -> Pin<Box<dyn Future<Output = Result<sqlx::postgres::PgQueryResult, sqlx::Error>> + Send + 'e>> {
        Box::pin(async move { query.execute(&*self).await })
    }

    fn fetch_all_query<'e>(
        &'e mut self,
        query: Query<'e, Postgres, PgArguments>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<sqlx::postgres::PgRow>, sqlx::Error>> + Send + 'e>> {
        Box::pin(async move { query.fetch_all(&*self).await })
    }

    fn fetch_optional_query<'e>(
        &'e mut self,
        query: Query<'e, Postgres, PgArguments>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<sqlx::postgres::PgRow>, sqlx::Error>> + Send + 'e>> {
        Box::pin(async move { query.fetch_optional(&*self).await })
    }
}

impl RangoExecutor for Transaction<'_, Postgres> {
    fn execute_query<'e>(
        &'e mut self,
        query: Query<'e, Postgres, PgArguments>,
    ) -> Pin<Box<dyn Future<Output = Result<sqlx::postgres::PgQueryResult, sqlx::Error>> + Send + 'e>> {
        Box::pin(async move { query.execute(&mut **self).await })
    }

    fn fetch_all_query<'e>(
        &'e mut self,
        query: Query<'e, Postgres, PgArguments>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<sqlx::postgres::PgRow>, sqlx::Error>> + Send + 'e>> {
        Box::pin(async move { query.fetch_all(&mut **self).await })
    }

    fn fetch_optional_query<'e>(
        &'e mut self,
        query: Query<'e, Postgres, PgArguments>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<sqlx::postgres::PgRow>, sqlx::Error>> + Send + 'e>> {
        Box::pin(async move { query.fetch_optional(&mut **self).await })
    }
}
