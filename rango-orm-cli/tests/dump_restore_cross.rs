//! SQLite dump restored into Postgres: the Postgres project's own migrations are recorded,
//! so `rango migrate` has nothing left to apply.
//!
//! Run with: `DATABASE_URL=postgres://postgres:postgres@localhost:5433/rango_test cargo test -p rango-orm-cli --test dump_restore_cross --features integration -- --test-threads=1`
#![cfg(feature = "integration")]

mod common;

use common::Project;

fn assert_ok(o: &std::process::Output) {
    assert!(
        o.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}

#[tokio::test]
async fn sqlite_dump_restores_into_postgres_with_the_target_migrations() {
    let pg_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5433/rango_test".into());
    let sqlite = Project::new("cross_src", "sqlite");
    let pg = Project::new("cross_dst", "postgres");
    let db = format!("sqlite://{}?mode=rwc", sqlite.dir.join("a.db").display());

    for p in [&sqlite, &pg] {
        assert_ok(&p.cli(None, &["makemigrations", "src", "--yes"]));
    }
    assert_ok(&sqlite.cli(Some(&db), &["migrate"]));
    assert_ok(&sqlite.cli(Some(&db), &["dump", "-o", "d.json"]));
    std::fs::copy(sqlite.dir.join("d.json"), pg.dir.join("d.json")).unwrap();

    // Start from an empty Postgres schema for the fixture tables.
    let dump: serde_json::Value =
        serde_json::from_slice(&std::fs::read(pg.dir.join("d.json")).unwrap()).unwrap();
    let pool = sqlx::PgPool::connect(&pg_url).await.unwrap();
    let mut tables: Vec<String> = dump["schema"]["tables"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    tables.push("_rango_migrations".into());
    for t in tables {
        sqlx::query(sqlx::AssertSqlSafe(&*format!(
            "DROP TABLE IF EXISTS \"{t}\" CASCADE"
        )))
        .execute(&pool)
        .await
        .unwrap();
    }

    let out = pg.cli(Some(&pg_url), &["restore", "d.json"]);
    assert_ok(&out);
    assert!(String::from_utf8_lossy(&out.stderr).contains("recorded as applied instead"));
    let out = pg.cli(Some(&pg_url), &["migrate"]);
    assert_ok(&out);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Nothing to apply"));
    pool.close().await;
}
