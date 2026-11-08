//! On-demand end-to-end scenario suite for a small, realistic blog/CMS app.
//!
//! NOT part of regular CI: it is gated behind the `integration-scenarios` feature,
//! so a plain `cargo test --workspace` compiles it to an empty crate and runs
//! nothing. Run it on demand, on SQLite in-memory (no Docker):
//!
//!   just scenarios
//!
//! (builds the `rango` CLI binary first, then runs the suite)
//!
//! It drives the product the way a real user would — define models, create the
//! schema, query, follow relations, use M2M, filter through a relation with a
//! subquery, transact, and bulk-insert — and asserts the observable behaviour.

#![cfg(feature = "integration-scenarios")]

use rango_orm_core::*;
use rango_orm_derive::Model;
use rango_orm_sqlite::*;
use uuid::Uuid;

// ── A real app's models ───────────────────────────────────────────────────────

#[derive(Model, Debug, Clone)]
#[model(table = "app_author")]
struct Author {
    id: FieldUuid,
    #[field(unique)]
    email: FieldEmail,
    name: FieldVarchar<1, 100>,
}

#[derive(Model, Debug, Clone)]
#[model(table = "app_post")]
struct Post {
    id: FieldUuid,
    author_id: ForeignKey<Author>,
    title: FieldVarchar<1, 200>,
    published: FieldBool,
    tags: ManyToMany<Tag>,
}

#[derive(Model, Debug, Clone)]
#[model(table = "app_comment")]
struct Comment {
    id: FieldUuid,
    post_id: ForeignKey<Post>,
    body: FieldText,
}

#[derive(Model, Debug, Clone)]
#[model(table = "app_tag")]
struct Tag {
    id: FieldUuid,
    name: FieldVarchar<1, 50>,
}

// ── Schema (hand-written DDL, matching integration_sqlite.rs) ────────────────

async fn setup(pool: &SqlitePool) {
    for ddl in [
        r#"CREATE TABLE "app_author" (
            "id"    TEXT PRIMARY KEY,
            "email" TEXT NOT NULL UNIQUE,
            "name"  TEXT NOT NULL
        )"#,
        r#"CREATE TABLE "app_post" (
            "id"        TEXT PRIMARY KEY,
            "author_id" TEXT NOT NULL REFERENCES "app_author"("id") ON DELETE CASCADE,
            "title"     TEXT NOT NULL,
            "published" INTEGER NOT NULL
        )"#,
        r#"CREATE TABLE "app_comment" (
            "id"      TEXT PRIMARY KEY,
            "post_id" TEXT NOT NULL REFERENCES "app_post"("id") ON DELETE CASCADE,
            "body"    TEXT NOT NULL
        )"#,
        r#"CREATE TABLE "app_tag" (
            "id"   TEXT PRIMARY KEY,
            "name" TEXT NOT NULL
        )"#,
        r#"CREATE TABLE "app_post_app_tag" (
            "app_post_id" TEXT NOT NULL REFERENCES "app_post"("id") ON DELETE CASCADE,
            "app_tag_id"  TEXT NOT NULL REFERENCES "app_tag"("id") ON DELETE CASCADE,
            PRIMARY KEY ("app_post_id", "app_tag_id")
        )"#,
    ] {
        sqlx::query(ddl)
            .execute(pool)
            .await
            .expect("schema setup failed");
    }
}

fn author(name: &str) -> Author {
    Author {
        id: FieldUuid(Uuid::new_v4()),
        email: FieldEmail::new(format!("{name}@blog.test")).unwrap(),
        name: FieldVarchar::new(name.to_string()).unwrap(),
    }
}

fn post(author_id: &Uuid, title: &str, published: bool) -> Post {
    Post {
        id: FieldUuid(Uuid::new_v4()),
        author_id: ForeignKey::new(*author_id),
        title: FieldVarchar::new(title.to_string()).unwrap(),
        published: FieldBool(published),
        tags: ManyToMany::empty(),
    }
}

// ── The scenario: one real app, driven end to end ────────────────────────────

#[tokio::test]
async fn blog_app_end_to_end() {
    let pool = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    setup(&pool).await;

    // ── Seed a realistic blog: 2 authors, 3 posts (1 draft), a comment, tags ──
    let ada = author("ada");
    let bob = author("bob");
    insert(&pool, ada.clone()).await.unwrap();
    insert(&pool, bob.clone()).await.unwrap();

    let p1 = post(&ada.id.0, "hello", true);
    let p2 = post(&ada.id.0, "draft", false);
    let p3 = post(&bob.id.0, "world", true);
    for p in [&p1, &p2, &p3] {
        insert(&pool, p.clone()).await.unwrap();
    }
    insert(
        &pool,
        Comment {
            id: FieldUuid(Uuid::new_v4()),
            post_id: ForeignKey::new(p1.id.0),
            body: FieldText("nice post".into()),
        },
    )
    .await
    .unwrap();

    let t_rust = Tag {
        id: FieldUuid(Uuid::new_v4()),
        name: FieldVarchar::new("rust").unwrap(),
    };
    let t_orm = Tag {
        id: FieldUuid(Uuid::new_v4()),
        name: FieldVarchar::new("orm").unwrap(),
    };
    insert(&pool, t_rust.clone()).await.unwrap();
    insert(&pool, t_orm.clone()).await.unwrap();
    M2M::<Post, Tag>::add(&pool, &p1, &t_rust).await.unwrap();
    M2M::<Post, Tag>::add(&pool, &p1, &t_orm).await.unwrap();

    // ── Query builder: filter, order, limit/offset, count, exists ────────────
    let pub_posts = Post::filter(&pool)
        .eq(Post::PUBLISHED, true)
        .order_by("title")
        .all()
        .await
        .unwrap();
    let titles: Vec<_> = pub_posts
        .iter()
        .map(|p| p.title.as_str().to_string())
        .collect();
    assert_eq!(titles, vec!["hello".to_string(), "world".to_string()]);

    assert_eq!(
        Post::filter(&pool)
            .eq(Post::PUBLISHED, false)
            .count()
            .await
            .unwrap(),
        1
    );
    assert!(Comment::filter(&pool).exists().await.unwrap());

    let page = Post::filter(&pool)
        .order_by("title")
        .limit(1)
        .offset(1)
        .all()
        .await
        .unwrap();
    assert_eq!(page.len(), 1);
    // Titles sorted asc are [draft, hello, world]; offset 1 skips `draft`.
    assert_eq!(page[0].title.as_str(), "hello");

    let got = Author::filter(&pool)
        .eq(Author::EMAIL, "ada@blog.test")
        .one()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.name.as_str(), "ada");
    assert!(
        Author::filter(&pool)
            .eq(Author::EMAIL, "nobody@blog.test")
            .one()
            .await
            .unwrap()
            .is_none()
    );

    // ── Relations: select_related (JOIN) + prefetch_related (batched) ────────
    let posts = Post::filter(&pool)
        .select_related(Post::AUTHOR_ID)
        .order_by("title")
        .all()
        .await
        .unwrap();
    let hello = posts.iter().find(|p| p.title.as_str() == "hello").unwrap();
    assert_eq!(hello.related::<Author>().name.as_str(), "ada");

    let authors = Author::filter(&pool)
        .prefetch_related(Post::AUTHOR_ID)
        .all()
        .await
        .unwrap();
    let ada_row = authors.iter().find(|a| a.name.as_str() == "ada").unwrap();
    assert_eq!(ada_row.prefetched::<Post>().len(), 2);

    // ── M2M ───────────────────────────────────────────────────────────────────
    let tags = M2M::<Post, Tag>::all(&pool, &p1).await.unwrap();
    let tag_names: Vec<_> = tags.iter().map(|t| t.name.as_str().to_string()).collect();
    assert!(tag_names.contains(&"rust".to_string()) && tag_names.contains(&"orm".to_string()));

    // ── Subquery: filter authors through posts with a correlated EXISTS ──────
    let with_published = Author::filter(&pool)
        .where_exists::<Post>(Post::AUTHOR_ID, |q| q.eq(Post::PUBLISHED, true))
        .all()
        .await
        .unwrap();
    let who: Vec<_> = with_published
        .iter()
        .map(|a| a.name.as_str().to_string())
        .collect();
    assert!(
        who.contains(&"ada".to_string()) && who.contains(&"bob".to_string()),
        "both authors have a published post: {who:?}"
    );

    // The generated SQL is an EXISTS subquery, not a JOIN.
    let sql = Author::filter(&pool)
        .where_exists::<Post>(Post::AUTHOR_ID, |q| q.eq(Post::PUBLISHED, true))
        .explain();
    assert!(
        sql.contains("EXISTS (SELECT 1 FROM"),
        "expected a correlated EXISTS, got: {sql}"
    );
    assert!(
        !sql.to_uppercase().contains("JOIN"),
        "expected no JOIN, got: {sql}"
    );

    // NOT EXISTS: an author with no published post at all.
    let lonely = author("lonely");
    insert(&pool, lonely.clone()).await.unwrap();
    let no_pub = Author::filter(&pool)
        .not()
        .where_exists::<Post>(Post::AUTHOR_ID, |q| q.eq(Post::PUBLISHED, true))
        .all()
        .await
        .unwrap();
    assert!(no_pub.iter().any(|a| a.name.as_str() == "lonely"));

    // ── Transactions: commit on Ok, roll back on Err ─────────────────────────
    let committed = atomic(&pool, |tx| {
        Box::pin(async move { insert(&mut **tx, post(&ada.id.0, "in-tx", true)).await })
    })
    .await
    .unwrap();
    assert!(
        get::<_, Post>(&pool, &committed.id.to_sql_value())
            .await
            .unwrap()
            .is_some()
    );

    let rolled_back = atomic(&pool, |tx| {
        Box::pin(async move {
            insert(&mut **tx, post(&ada.id.0, "rolled-back", false)).await?;
            Err::<(), _>(Error::abort("simulated failure"))
        })
    })
    .await;
    assert!(rolled_back.is_err());
    assert_eq!(
        Post::filter(&pool)
            .eq(Post::TITLE, "rolled-back")
            .count()
            .await
            .unwrap(),
        0,
        "a failed transaction must not persist the insert"
    );

    // ── Bulk insert ───────────────────────────────────────────────────────────
    let bulk_comments: Vec<Comment> = (0..5)
        .map(|i| Comment {
            id: FieldUuid(Uuid::new_v4()),
            post_id: ForeignKey::new(p1.id.0),
            body: FieldText(format!("bulk comment {i}")),
        })
        .collect();
    assert_eq!(bulk_create(&pool, &bulk_comments).await.unwrap(), 5);
    assert_eq!(Comment::filter(&pool).count().await.unwrap(), 6); // 1 seeded + 5
}

/// Locate the built `rango` binary. `CARGO_BIN_EXE_rango` is not exposed for a
/// bin-only dev-dependency across packages, so fall back to the cargo layout:
/// the test binary is `target/debug/deps/scenarios-<hash>`, `rango` is `target/debug/rango`.
fn rango_bin() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_rango") {
        return std::path::PathBuf::from(p);
    }
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .map(|p| p.join("rango"))
        .expect("cannot locate the rango binary")
}

/// dump/restore round-trip through the real `rango` binary — the CLI surface of the
/// product, exercised end to end on a file-backed SQLite database in a throwaway
/// project dir (mirrors `rango-orm-cli/tests/dump_restore_sqlite.rs`).
#[tokio::test]
async fn blog_app_dump_restore_roundtrip() {
    use sqlx::Connection;

    let dir = std::env::temp_dir().join(format!(
        "rango_scenarios_dump_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("rango.toml"), "[database]\nbackend = \"sqlite\"\n").unwrap();
    std::fs::write(
        dir.join("src/models.rs"),
        r#"use rango::prelude::*;

#[derive(Model)]
#[model(table = "s_author")]
pub struct Author {
    pub id: FieldUuid,
    pub name: FieldText,
}

#[derive(Model)]
#[model(table = "s_post")]
pub struct Post {
    pub id: FieldUuid,
    pub author: ForeignKey<Author>,
    pub title: FieldText,
}
"#,
    )
    .unwrap();

    let url = |db: &str| format!("sqlite://{}?mode=rwc", dir.join(db).display());
    let cli = |db: &str, args: &[&str]| -> std::process::Output {
        std::process::Command::new(rango_bin())
            .current_dir(&dir)
            .env("DATABASE_URL", url(db))
            .stdin(std::process::Stdio::null())
            .args(args)
            .output()
            .unwrap()
    };
    let ok = |o: std::process::Output| {
        assert!(
            o.status.success(),
            "rango failed: stdout={} stderr={}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        o
    };

    // Migrate the blog app into a.db, then seed a couple of rows.
    ok(cli("a.db", &["makemigrations", "src", "--yes"]));
    ok(cli("a.db", &["migrate"]));
    {
        let mut c = sqlx::SqliteConnection::connect(url("a.db").as_str())
            .await
            .unwrap();
        sqlx::query("INSERT INTO s_author (id, name) VALUES ('a1', 'Ada'), ('a2', 'Bob')")
            .execute(&mut c)
            .await
            .unwrap();
        sqlx::query("INSERT INTO s_post (id, author, title) VALUES ('p1', 'a1', 'Hello')")
            .execute(&mut c)
            .await
            .unwrap();
    }

    // Dump schema + data to a Rango JSON file.
    ok(cli("a.db", &["dump", "-o", "blog.json"]));

    // Restore into a fresh, empty database: the tables and rows come back.
    ok(cli("b.db", &["restore", "blog.json"]));
    {
        let mut c = sqlx::SqliteConnection::connect(url("b.db").as_str())
            .await
            .unwrap();
        let authors: i64 = sqlx::query_scalar("SELECT count(*) FROM s_author")
            .fetch_one(&mut c)
            .await
            .unwrap();
        let posts: i64 = sqlx::query_scalar("SELECT count(*) FROM s_post")
            .fetch_one(&mut c)
            .await
            .unwrap();
        assert_eq!(authors, 2, "restore must bring the authors back");
        assert_eq!(posts, 1, "restore must bring the posts back");
    }

    let _ = std::fs::remove_dir_all(&dir);
}
