//! A real project on SQLite: `rango makemigrations` + `rango migrate` on the fixture models,
//! then queries with the compiled models. No server needed.

mod common;
#[path = "fixtures/models.rs"]
mod models;

crate::scenario!(rango_orm_sqlite, SqlitePool);

use common::Project;

#[tokio::test]
async fn real_project_sqlite() {
    let p = Project::new("real_sqlite", "sqlite");
    let db = p.dir.join("rp.db");
    let url = format!("sqlite://{}?mode=rwc", db.display());
    let drop_all = |_tables: Vec<String>| {
        let db = db.clone();
        async move {
            let _ = std::fs::remove_file(db);
        }
    };
    let exercise = || async {
        let pool = rango_orm_sqlite::connect(&url).await.unwrap();
        scenario::run(&pool).await;
        pool.close().await;
    };
    common::flow(&p, &url, drop_all, exercise).await;
}
