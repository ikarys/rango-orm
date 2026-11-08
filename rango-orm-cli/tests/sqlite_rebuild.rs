//! SQLite table rebuild: a change SQLite cannot `ALTER` in place recreates the table,
//! keeps its rows, and never cascades into child tables.

mod common;

use common::{Project, assert_ok};
use sqlx::{Connection, SqliteConnection};

const V1: &str = r#"use rango::prelude::*;

#[derive(Model)]
#[model(table = "rb_author")]
pub struct Author {
    pub id: FieldUuid,
    pub name: Option<FieldText>,
}

#[derive(Model)]
#[model(table = "rb_book")]
pub struct Book {
    pub id: FieldUuid,
    pub title: FieldText,
    pub author: ForeignKey<Author>,
}
"#;

#[tokio::test]
async fn sqlite_rebuild_keeps_rows_and_children() {
    let p = Project::new("sqlite_rebuild", "sqlite");
    let url = format!("sqlite://{}?mode=rwc", p.dir.join("rb.db").display());
    let models = p.dir.join("src/models.rs");

    std::fs::write(&models, V1).unwrap();
    assert_ok(&p.cli(None, &["makemigrations", "src", "--yes"]));
    assert_ok(&p.cli(Some(&url), &["migrate"]));

    let mut c = SqliteConnection::connect(&url).await.unwrap();
    for sql in [
        "INSERT INTO rb_author (id, name) VALUES ('a1', 'Ada'), ('a2', 'Bob')",
        "INSERT INTO rb_book (id, title, author) VALUES ('b1', 'Notes', 'a1')",
    ] {
        sqlx::query(sql).execute(&mut c).await.unwrap();
    }

    // name: nullable -> NOT NULL + unique. Neither is possible with SQLite's ALTER TABLE.
    std::fs::write(
        &models,
        V1.replace(
            "pub name: Option<FieldText>,",
            "#[field(unique)]\n    pub name: FieldText,",
        ),
    )
    .unwrap();
    assert_ok(&p.cli(None, &["makemigrations", "src", "--yes"]));
    let sql = p.migrations();
    assert!(
        sql.contains("ALTER TABLE \"_rango_new_rb_author\" RENAME TO \"rb_author\""),
        "{sql}"
    );
    assert_ok(&p.cli(Some(&url), &["migrate"]));

    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM rb_author ORDER BY name")
        .fetch_all(&mut c)
        .await
        .unwrap();
    assert_eq!(names, ["Ada", "Bob"]);
    // DROP TABLE of the parent ran with foreign keys off: the book is still there.
    let books: i64 = sqlx::query_scalar("SELECT count(*) FROM rb_book")
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert_eq!(books, 1);

    // The new constraints are enforced.
    let dup = sqlx::query("INSERT INTO rb_author (id, name) VALUES ('a3', 'Ada')")
        .execute(&mut c)
        .await;
    assert!(dup.is_err(), "unique not enforced");
    let null = sqlx::query("INSERT INTO rb_author (id, name) VALUES ('a4', NULL)")
        .execute(&mut c)
        .await;
    assert!(null.is_err(), "NOT NULL not enforced");

    // The child's FK still points at the rebuilt table and still cascades.
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut c)
        .await
        .unwrap();
    sqlx::query("DELETE FROM rb_author WHERE id = 'a1'")
        .execute(&mut c)
        .await
        .unwrap();
    let books: i64 = sqlx::query_scalar("SELECT count(*) FROM rb_book")
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert_eq!(books, 0);
}

#[tokio::test]
async fn sqlite_rebuild_rolls_back_on_null_rows() {
    let p = Project::new("sqlite_rebuild_null", "sqlite");
    let url = format!("sqlite://{}?mode=rwc", p.dir.join("rb.db").display());
    let models = p.dir.join("src/models.rs");

    std::fs::write(&models, V1).unwrap();
    assert_ok(&p.cli(None, &["makemigrations", "src", "--yes"]));
    assert_ok(&p.cli(Some(&url), &["migrate"]));
    let mut c = SqliteConnection::connect(&url).await.unwrap();
    sqlx::query("INSERT INTO rb_author (id, name) VALUES ('a1', NULL)")
        .execute(&mut c)
        .await
        .unwrap();

    std::fs::write(
        &models,
        V1.replace("pub name: Option<FieldText>,", "pub name: FieldText,"),
    )
    .unwrap();
    assert_ok(&p.cli(None, &["makemigrations", "src", "--yes"]));
    let out = p.cli(Some(&url), &["migrate"]);
    assert!(
        !out.status.success(),
        "a NULL row must fail the NOT NULL rebuild"
    );

    // Rolled back: the original table and its row are intact, no temp table left.
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM rb_author")
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert_eq!(rows, 1);
    let temp: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE name = '_rango_new_rb_author'",
    )
    .fetch_one(&mut c)
    .await
    .unwrap();
    assert_eq!(temp, 0);
}

#[test]
fn migrate_prefers_migrate_database_url() {
    let p = Project::new("sqlite_migrate_url", "sqlite");
    std::fs::write(p.dir.join("src/models.rs"), V1).unwrap();
    assert_ok(&p.cli(None, &["makemigrations", "src", "--yes"]));
    let db = p.dir.join("ddl.db");
    // DATABASE_URL (the app's DML role) points nowhere usable: migrate must not touch it.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rango"))
        .current_dir(&p.dir)
        .env("DATABASE_URL", "sqlite:///nonexistent/dir/app.db")
        .env(
            "MIGRATE_DATABASE_URL",
            format!("sqlite://{}?mode=rwc", db.display()),
        )
        .arg("migrate")
        .output()
        .unwrap();
    assert_ok(&out);
    assert!(db.exists());
}
