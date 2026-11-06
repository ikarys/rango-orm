//! A real project on Postgres: `rango makemigrations` + `rango migrate` on the fixture models,
//! then queries with the compiled models.
//!
//! Run with: `DATABASE_URL=postgres://postgres:postgres@localhost:5433/rango_test cargo test -p rango-orm-cli --test real_project_postgres --features integration -- --test-threads=1`
#![cfg(feature = "integration")]

mod common;
#[path = "fixtures/models.rs"]
mod models;

crate::scenario!(rango_orm_postgres, PgPool);

use common::Project;

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5433/rango_test".into())
}

#[tokio::test]
async fn real_project_postgres() {
    let p = Project::new("real_pg", "postgres");
    let url = database_url();
    let drop_all = |tables: Vec<String>| {
        let url = url.clone();
        async move {
            let pool = sqlx::PgPool::connect(&url).await.unwrap();
            for t in tables {
                sqlx::query(&format!("DROP TABLE IF EXISTS \"{t}\" CASCADE"))
                    .execute(&pool)
                    .await
                    .unwrap();
            }
            pool.close().await;
        }
    };
    let exercise = || async {
        let pool = rango_orm_postgres::connect(&rango::DatabaseConfig::from_url(url.clone()))
            .await
            .unwrap();
        scenario::run(&pool).await;
        pool.close().await;
    };
    common::flow(&p, &url, drop_all, exercise).await;
}
