//! A real project on MySQL / MariaDB: `rango makemigrations` + `rango migrate` on the fixture
//! models, then queries with the compiled models.
//!
//! Run with: `MYSQL_URL=mysql://root:rango@127.0.0.1:3307/rango_test cargo test -p rango-orm-cli --test real_project_mysql --features integration-mysql -- --test-threads=1`
#![cfg(feature = "integration-mysql")]

mod common;
#[path = "fixtures/models.rs"]
mod models;

crate::scenario!(rango_orm_mysql, MySqlPool);

use common::Project;

fn database_url() -> String {
    std::env::var("MYSQL_URL")
        .unwrap_or_else(|_| "mysql://root:rango@127.0.0.1:3307/rango_test".into())
}

#[tokio::test]
async fn real_project_mysql() {
    let p = Project::new("real_mysql", "mysql");
    let url = database_url();
    let drop_all = |tables: Vec<String>| {
        let url = url.clone();
        async move {
            let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
            let mut sql = String::from("SET FOREIGN_KEY_CHECKS = 0; ");
            for t in tables {
                sql.push_str(&format!("DROP TABLE IF EXISTS `{t}`; "));
            }
            sql.push_str("SET FOREIGN_KEY_CHECKS = 1;");
            sqlx::raw_sql(sqlx::AssertSqlSafe(&*sql))
                .execute(&pool)
                .await
                .unwrap();
            pool.close().await;
        }
    };
    let exercise = || async {
        let pool = rango_orm_mysql::connect(&url).await.unwrap();
        scenario::run(&pool).await;
        pool.close().await;
    };
    common::flow(&p, &url, drop_all, exercise).await;
}
