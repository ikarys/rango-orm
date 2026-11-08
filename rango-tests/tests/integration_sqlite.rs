/// SQLite integration tests — run fully in-memory, no Docker needed.
///
/// Run with:
///   cargo test --test integration_sqlite --features rango-tests/integration-sqlite
use rango_orm_core::*;
use rango_orm_derive::Model;
use rango_orm_sqlite::*;
use uuid::Uuid;

// ── Test models ───────────────────────────────────────────────────────────────

#[derive(Model, Debug, Clone, PartialEq)]
#[model(table = "test_user")]
struct TestUser {
    id: FieldUuid,
    #[field(unique)]
    email: FieldEmail,
    name: FieldVarchar<1, 100>,
    active: FieldBool,
    score: Option<FieldInt>,
}

#[derive(Model, Debug, Clone)]
#[model(table = "test_post")]
struct TestPost {
    id: FieldUuid,
    author_id: ForeignKey<TestUser>,
    title: FieldVarchar<1, 100>,
}

#[derive(Model, Debug, Clone)]
#[model(table = "test_blog")]
struct TestBlog {
    id: FieldUuid,
    title: FieldVarchar<1, 100>,
    tags: ManyToMany<TestTag>,
}

#[derive(Model, Debug, Clone)]
#[model(table = "test_tag")]
struct TestTag {
    id: FieldUuid,
    name: FieldVarchar<1, 100>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn pool() -> SqlitePool {
    SqlitePool::connect("sqlite::memory:")
        .await
        .expect("Failed to create in-memory SQLite pool")
}

async fn setup(pool: &SqlitePool) {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS "test_user" (
            "id"     TEXT PRIMARY KEY,
            "email"  TEXT NOT NULL UNIQUE,
            "name"   TEXT NOT NULL,
            "active" INTEGER NOT NULL,
            "score"  INTEGER
        )"#,
    )
    .execute(pool)
    .await
    .expect("setup failed");
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_prefetch_related_many_to_many() {
    let pool = pool().await;

    sqlx::query(
        r#"CREATE TABLE "test_blog" (
            "id"    TEXT PRIMARY KEY,
            "title" TEXT NOT NULL
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"CREATE TABLE "test_tag" (
            "id"   TEXT PRIMARY KEY,
            "name" TEXT NOT NULL
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"CREATE TABLE "test_blog_test_tag" (
            "test_blog_id" TEXT NOT NULL,
            "test_tag_id"  TEXT NOT NULL,
            PRIMARY KEY ("test_blog_id", "test_tag_id")
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let b1 = TestBlog {
        id: FieldUuid(Uuid::new_v4()),
        title: FieldVarchar::new("rust".to_string()).unwrap(),
        tags: ManyToMany::empty(),
    };
    let b2 = TestBlog {
        id: FieldUuid(Uuid::new_v4()),
        title: FieldVarchar::new("empty".to_string()).unwrap(),
        tags: ManyToMany::empty(),
    };
    let t1 = TestTag {
        id: FieldUuid(Uuid::new_v4()),
        name: FieldVarchar::new("orm".to_string()).unwrap(),
    };
    let t2 = TestTag {
        id: FieldUuid(Uuid::new_v4()),
        name: FieldVarchar::new("async".to_string()).unwrap(),
    };

    insert(&pool, b1.clone()).await.unwrap();
    insert(&pool, b2.clone()).await.unwrap();
    insert(&pool, t1.clone()).await.unwrap();
    insert(&pool, t2.clone()).await.unwrap();

    M2M::<TestBlog, TestTag>::add(&pool, &b1, &t1)
        .await
        .unwrap();
    M2M::<TestBlog, TestTag>::add(&pool, &b1, &t2)
        .await
        .unwrap();

    let blogs = TestBlog::filter(&pool)
        .prefetch_related(TestBlog::TAGS)
        .all()
        .await
        .unwrap();

    assert_eq!(blogs.len(), 2);

    let b1_blog = blogs.iter().find(|b| b.id.0 == b1.id.0).unwrap();
    let b2_blog = blogs.iter().find(|b| b.id.0 == b2.id.0).unwrap();

    let mut tag_names: Vec<String> = b1_blog
        .prefetched::<TestTag>()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    tag_names.sort();
    assert_eq!(tag_names, vec!["async".to_string(), "orm".to_string()]);
    assert_eq!(b2_blog.prefetched::<TestTag>().len(), 0);

    // M2M ops: set replaces atomically, exists, reverse, idempotent add, remove.
    M2M::<TestBlog, TestTag>::set(&pool, &b2, std::slice::from_ref(&t1))
        .await
        .unwrap();
    assert!(
        M2M::<TestBlog, TestTag>::exists(&pool, &b2, &t1)
            .await
            .unwrap()
    );
    assert_eq!(
        M2M::<TestBlog, TestTag>::reverse(&pool, &t1)
            .await
            .unwrap()
            .len(),
        2
    );
    M2M::<TestBlog, TestTag>::add(&pool, &b2, &t1)
        .await
        .unwrap();
    M2M::<TestBlog, TestTag>::remove(&pool, &b1, &t1)
        .await
        .unwrap();
    assert_eq!(
        M2M::<TestBlog, TestTag>::all(&pool, &b1)
            .await
            .unwrap()
            .len(),
        1
    );
}

fn user(email: &str) -> TestUser {
    TestUser {
        id: FieldUuid(Uuid::new_v4()),
        email: FieldEmail::new(email.to_string()).unwrap(),
        name: FieldVarchar::new("Test User".to_string()).unwrap(),
        active: FieldBool(true),
        score: None,
    }
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_select_and_prefetch_related() {
    let pool = pool().await;
    setup(&pool).await;
    sqlx::query(
        r#"CREATE TABLE "test_post" (
            "id"        TEXT PRIMARY KEY,
            "author_id" TEXT NOT NULL REFERENCES "test_user" ("id"),
            "title"     TEXT NOT NULL
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let alice = insert(&pool, user("alice@sqlite.com")).await.unwrap();
    let bob = insert(&pool, user("bob@sqlite.com")).await.unwrap();
    for title in ["first", "second"] {
        let post = TestPost {
            id: FieldUuid(Uuid::new_v4()),
            author_id: ForeignKey::new(alice.id.0),
            title: FieldVarchar::new(title.to_string()).unwrap(),
        };
        insert(&pool, post).await.unwrap();
    }

    let mut q = TestPost::filter(&pool)
        .select_related(TestPost::AUTHOR_ID)
        .order_by("-title")
        .limit(1);
    assert!(q.explain().contains("LEFT JOIN"), "{}", q.explain());
    let plan = q.explain_analyze().await.unwrap();
    assert!(plan.contains("SCAN") || plan.contains("SEARCH"), "{plan}");
    let posts = q.all().await.unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].title.as_str(), "second");
    assert_eq!(posts[0].related::<TestUser>().email, alice.email);

    let users = TestUser::filter(&pool)
        .prefetch_related(TestPost::AUTHOR_ID)
        .all()
        .await
        .unwrap();
    let posts_of = |u: &TestUser| {
        users
            .iter()
            .find(|w| w.id == u.id)
            .unwrap()
            .prefetched::<TestPost>()
            .len()
    };
    assert_eq!(posts_of(&alice), 2);
    assert_eq!(posts_of(&bob), 0);
}

// ── CRUD ──────────────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_insert_and_get() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("insert@sqlite.com");
    let inserted = insert(&pool, u.clone()).await.expect("insert failed");
    assert_eq!(inserted.email, u.email);

    let fetched = get::<_, TestUser>(&pool, &u.id.to_sql_value())
        .await
        .expect("get failed")
        .expect("user not found");
    assert_eq!(fetched.email, u.email);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_update() {
    let pool = pool().await;
    setup(&pool).await;

    let mut u = user("update@sqlite.com");
    insert(&pool, u.clone()).await.unwrap();

    u.name = FieldVarchar::new("Updated".to_string()).unwrap();
    let updated = update(&pool, u.clone()).await.expect("update failed");
    assert_eq!(
        updated.name,
        FieldVarchar::new("Updated".to_string()).unwrap()
    );
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_delete() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("delete@sqlite.com");
    insert(&pool, u.clone()).await.unwrap();
    delete(&pool, &u).await.expect("delete failed");

    let fetched = get::<_, TestUser>(&pool, &u.id.to_sql_value())
        .await
        .unwrap();
    assert!(fetched.is_none());
}

// ── Query builder ─────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_filter_eq() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("active@sqlite.com")).await.unwrap();
    let mut inactive = user("inactive@sqlite.com");
    inactive.active = FieldBool(false);
    insert(&pool, inactive).await.unwrap();

    let results = TestUser::filter(&pool)
        .eq("active", 1i32) // SQLite stores bool as INTEGER
        .all()
        .await
        .expect("filter failed");

    assert!(
        results
            .iter()
            .any(|u| u.email == FieldEmail::new("active@sqlite.com".to_string()).unwrap())
    );
    assert!(
        !results
            .iter()
            .any(|u| u.email == FieldEmail::new("inactive@sqlite.com".to_string()).unwrap())
    );
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_subqueries() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("a@sqlite.com")).await.unwrap();
    let mut inactive = user("b@sqlite.com");
    inactive.active = FieldBool(false);
    insert(&pool, inactive).await.unwrap();

    // Self-referencing EXISTS: exercises the inner-table alias and bind order.
    let active = TestUser::filter(&pool)
        .eq("email", "b@sqlite.com".to_string())
        .or()
        .where_exists::<TestUser>("id", |q| q.eq("active", 1i32))
        .order_by("email")
        .all()
        .await
        .unwrap();
    assert_eq!(
        active.iter().map(|u| u.email.as_str()).collect::<Vec<_>>(),
        ["a@sqlite.com", "b@sqlite.com"]
    );

    let inactive = TestUser::filter(&pool)
        .not()
        .in_subquery(TestUser::ID, TestUser::ID, |q| q.eq("active", 1i32))
        .all()
        .await
        .unwrap();
    assert_eq!(
        inactive
            .iter()
            .map(|u| u.email.as_str())
            .collect::<Vec<_>>(),
        ["b@sqlite.com"]
    );
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_count_and_exists() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("count1@sqlite.com")).await.unwrap();
    insert(&pool, user("count2@sqlite.com")).await.unwrap();

    let count = TestUser::filter(&pool).count().await.unwrap();
    assert!(count >= 2);

    let exists = TestUser::filter(&pool)
        .eq("email", "count1@sqlite.com")
        .exists()
        .await
        .unwrap();
    assert!(exists);
}

// ── Bulk ops ──────────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_bulk_create() {
    let pool = pool().await;
    setup(&pool).await;

    let users: Vec<TestUser> = (0..5)
        .map(|i| user(&format!("bulk{}@sqlite.com", i)))
        .collect();

    let n = bulk_create(&pool, &users)
        .await
        .expect("bulk_create failed");
    assert_eq!(n, 5);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_bulk_create_is_atomic_across_chunks() {
    let pool = pool().await;
    setup(&pool).await;

    // Enough rows for several chunks; the last row duplicates the first email.
    let mut users: Vec<TestUser> = (0..6500)
        .map(|i| user(&format!("atomic{}@t.com", i)))
        .collect();
    users.push(user("atomic0@t.com"));

    let err = bulk_create(&pool, &users).await;
    assert!(err.is_err());
    assert_eq!(TestUser::filter(&pool).count().await.unwrap(), 0);
}

// ── Transactions ──────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_atomic_commit() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("atomic@sqlite.com");
    let mut tx = pool.begin().await.unwrap();
    insert(&mut *tx, u).await.expect("insert in tx failed");
    tx.commit().await.unwrap();

    let exists = TestUser::filter(&pool)
        .eq("email", "atomic@sqlite.com")
        .exists()
        .await
        .unwrap();
    assert!(exists);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_atomic_rollback() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("rollback@sqlite.com");
    let mut tx = pool.begin().await.unwrap();
    insert(&mut *tx, u).await.expect("insert in tx failed");
    tx.rollback().await.unwrap();

    let exists = TestUser::filter(&pool)
        .eq("email", "rollback@sqlite.com")
        .exists()
        .await
        .unwrap();
    assert!(!exists);
}

// ── Aggregations ──────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_aggregations() {
    let pool = pool().await;
    setup(&pool).await;

    let mut u1 = user("agg1@sqlite.com");
    u1.score = Some(FieldInt(10));
    let mut u2 = user("agg2@sqlite.com");
    u2.score = Some(FieldInt(20));
    let mut u3 = user("agg3@sqlite.com");
    u3.score = Some(FieldInt(30));
    bulk_create(&pool, &[u1, u2, u3]).await.unwrap();

    let sum = TestUser::filter(&pool).sum("score").await.unwrap();
    assert_eq!(sum, Some(60.0));

    let avg = TestUser::filter(&pool).avg("score").await.unwrap();
    assert_eq!(avg, Some(20.0));

    let min = TestUser::filter(&pool).min::<i64>("score").await.unwrap();
    assert_eq!(min, Some(10));

    let max = TestUser::filter(&pool).max::<i64>("score").await.unwrap();
    assert_eq!(max, Some(30));
}

// ── Raw SQL ───────────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_raw_scalar() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("raw@sqlite.com")).await.unwrap();

    let count: i64 = raw_scalar(
        &pool,
        "SELECT COUNT(*) FROM test_user WHERE email = ?",
        vec![SqlValue::Text("raw@sqlite.com".to_string())],
    )
    .await
    .expect("raw_scalar failed");

    assert_eq!(count, 1);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_values_projection() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("values@sqlite.com")).await.unwrap();

    let rows = TestUser::filter(&pool)
        .eq("email", "values@sqlite.com")
        .values(&["email", "active"])
        .await
        .expect("values failed");

    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains_key("email"));
    assert!(rows[0].contains_key("active"));
    assert!(!rows[0].contains_key("name"));

    if let SqlValue::Text(email) = &rows[0]["email"] {
        assert_eq!(email, "values@sqlite.com");
    } else {
        panic!("email should be SqlValue::Text");
    }
}

// ── Export / Import ───────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_export_json() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("export1@sqlite.com")).await.unwrap();
    insert(&pool, user("export2@sqlite.com")).await.unwrap();

    // Export via raw query — mirrors what rango export does
    let rows = TestUser::filter(&pool)
        .values(&["id", "email", "active"])
        .await
        .expect("values failed");

    assert_eq!(rows.len(), 2);

    // Serialize to JSON
    let json = serde_json::to_string(
        &rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|(k, v)| {
                        let jv = match v {
                            rango_orm_core::SqlValue::Text(s) => {
                                serde_json::Value::String(s.clone())
                            }
                            rango_orm_core::SqlValue::BigInt(n) => {
                                serde_json::Value::Number((*n).into())
                            }
                            _ => serde_json::Value::Null,
                        };
                        (k.clone(), jv)
                    })
                    .collect::<serde_json::Map<_, _>>()
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();

    assert!(json.contains("export1@sqlite.com"));
    assert!(json.contains("export2@sqlite.com"));
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_roundtrip() {
    // Insert → export via values() → verify data integrity
    let pool = pool().await;
    setup(&pool).await;

    let u = user("roundtrip@sqlite.com");
    insert(&pool, u.clone()).await.unwrap();

    let rows = TestUser::filter(&pool)
        .eq("email", "roundtrip@sqlite.com")
        .values(&["email", "name"])
        .await
        .expect("values failed");

    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("email"),
        Some(&rango_orm_core::SqlValue::Text(
            "roundtrip@sqlite.com".to_string()
        ))
    );
    assert_eq!(
        rows[0].get("name"),
        Some(&rango_orm_core::SqlValue::Text("Test User".to_string()))
    );
}

// ── SqliteQueryExt ────────────────────────────────────────────────────────────

use rango_orm_sqlite::SqliteQueryExt;

#[derive(rango_orm_derive::Model, Debug, Clone)]
#[model(table = "test_article_sqlite")]
struct TestArticleSqlite {
    id: FieldUuid,
    title: FieldVarchar<1, 255>,
    metadata: FieldText, // JSON stored as TEXT in SQLite
}

async fn setup_article(pool: &SqlitePool) {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS "test_article_sqlite" (
            "id"       TEXT PRIMARY KEY,
            "title"    TEXT NOT NULL,
            "metadata" TEXT NOT NULL DEFAULT '{}'
        )"#,
    )
    .execute(pool)
    .await
    .expect("setup_article failed");

    sqlx::query(r#"TRUNCATE "test_article_sqlite""#)
        .execute(pool)
        .await
        .ok(); // SQLite doesn't support TRUNCATE — ignore

    sqlx::query(r#"DELETE FROM "test_article_sqlite""#)
        .execute(pool)
        .await
        .ok();
}

fn article_sqlite(title: &str, metadata: &str) -> TestArticleSqlite {
    TestArticleSqlite {
        id: FieldUuid(uuid::Uuid::new_v4()),
        title: FieldVarchar::new(title.to_string()).unwrap(),
        metadata: FieldText(metadata.to_string()),
    }
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_json_extract_eq() {
    let pool = pool().await;
    setup_article(&pool).await;

    insert(
        &pool,
        article_sqlite("Published", r#"{"status":"published"}"#),
    )
    .await
    .unwrap();
    insert(&pool, article_sqlite("Draft", r#"{"status":"draft"}"#))
        .await
        .unwrap();

    let results = TestArticleSqlite::filter(&pool)
        .json_extract_eq("metadata", "$.status", "published")
        .all()
        .await
        .expect("json_extract_eq failed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title.as_str(), "Published");
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_json_extract_like() {
    let pool = pool().await;
    setup_article(&pool).await;

    insert(
        &pool,
        article_sqlite("Rust ORM", r#"{"tags":"rust,orm,async"}"#),
    )
    .await
    .unwrap();
    insert(
        &pool,
        article_sqlite("Python web", r#"{"tags":"python,flask"}"#),
    )
    .await
    .unwrap();

    let results = TestArticleSqlite::filter(&pool)
        .json_extract_like("metadata", "$.tags", "%rust%")
        .all()
        .await
        .expect("json_extract_like failed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title.as_str(), "Rust ORM");
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_json_extract_exists() {
    let pool = pool().await;
    setup_article(&pool).await;

    insert(&pool, article_sqlite("With views", r#"{"views":42}"#))
        .await
        .unwrap();
    insert(&pool, article_sqlite("No views", r#"{"status":"draft"}"#))
        .await
        .unwrap();

    let results = TestArticleSqlite::filter(&pool)
        .json_extract_exists("metadata", "$.views")
        .all()
        .await
        .expect("json_extract_exists failed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title.as_str(), "With views");
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_unique_violation_is_typed() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("dup@sqlite.com")).await.unwrap();
    let err = insert(&pool, user("dup@sqlite.com")).await.unwrap_err();
    assert!(
        matches!(&err, Error::UniqueViolation { constraint: Some(c) } if c.contains("email")),
        "{err:?}"
    );
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_missing_table_is_typed() {
    let pool = pool().await; // no setup: the table does not exist
    let err = TestUser::filter(&pool)
        .all()
        .await
        .err()
        .expect("table is missing");
    assert!(
        matches!(&err, Error::TableMissing { table } if !table.is_empty()),
        "{err:?}"
    );
    assert!(err.to_string().contains("rango migrate"), "{err}");
}

// ── Typed columns ─────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_typed_columns() {
    let pool = pool().await;
    setup(&pool).await;

    let mut a = user("a@typed.com");
    a.score = Some(FieldInt(1));
    let mut b = user("b@typed.com");
    b.score = Some(FieldInt(2));
    b.active = FieldBool(false);
    bulk_create(&pool, &[a, b]).await.unwrap();

    assert_eq!(TestUser::EMAIL.name(), "email");

    let actives = TestUser::filter(&pool)
        .eq(TestUser::ACTIVE, true)
        .all()
        .await
        .unwrap();
    assert_eq!(actives.len(), 1);

    let desc = TestUser::filter(&pool)
        .order_by(TestUser::SCORE.desc())
        .all()
        .await
        .unwrap();
    assert_eq!(
        desc[0].email,
        FieldEmail::new("b@typed.com".to_string()).unwrap()
    );

    let sum = TestUser::filter(&pool).sum(TestUser::SCORE).await.unwrap();
    assert_eq!(sum, Some(3.0));
}

// ── Auto-increment PK ─────────────────────────────────────────────────────────

#[derive(Model, Debug, Clone)]
#[model(table = "test_counter")]
struct TestCounter {
    id: FieldAutoInt,
    label: FieldText,
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_auto_pk_is_generated() {
    let pool = pool().await;
    sqlx::query(r#"CREATE TABLE "test_counter" ("id" INTEGER PRIMARY KEY, "label" TEXT NOT NULL)"#)
        .execute(&pool)
        .await
        .unwrap();
    let new = |l: &str| TestCounter {
        id: FieldAutoInt::default(),
        label: FieldText(l.into()),
    };

    let a = insert(&pool, new("a")).await.unwrap();
    let b = insert(&pool, new("b")).await.unwrap();
    assert_eq!((a.id.0, b.id.0), (Some(1), Some(2)));

    assert_eq!(bulk_create(&pool, &[new("c"), new("d")]).await.unwrap(), 2);
    let d = get::<_, TestCounter>(&pool, &4i32.into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(d.label.0, "d");
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_unknown_column_is_rejected() {
    // Rejected before any SQL runs, so no table setup is needed.
    let pool = pool().await;
    let invalid = |r: Result<(), Error>| match r {
        Err(Error::Invalid(msg)) => assert!(msg.contains("nope"), "{msg}"),
        other => panic!("expected Error::Invalid, got {other:?}"),
    };
    invalid(
        TestUser::filter(&pool)
            .eq("nope", 1)
            .all()
            .await
            .map(|_| ()),
    );
    invalid(
        TestUser::filter(&pool)
            .order_by("-nope")
            .count()
            .await
            .map(|_| ()),
    );
    invalid(
        TestUser::filter(&pool)
            .group(|q| q.is_null("nope"))
            .exists()
            .await
            .map(|_| ()),
    );
    invalid(TestUser::filter(&pool).sum("nope").await.map(|_| ()));
    invalid(
        TestUser::filter(&pool)
            .values(&["email", "nope"])
            .await
            .map(|_| ()),
    );
}

// ── Implicit primary key: #[rango::model] ─────────────────────────────────────

#[rango::model]
#[derive(Debug, Clone)]
struct ImplicitUuid {
    title: FieldVarchar<1, 100>,
}

#[rango::model(pk = "bigint")]
#[derive(Debug, Clone)]
struct ImplicitBig {
    name: FieldText,
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_implicit_pk_uuid() {
    let pool = pool().await;
    sqlx::query(r#"CREATE TABLE "implicit_uuid" ("id" TEXT PRIMARY KEY, "title" TEXT NOT NULL)"#)
        .execute(&pool)
        .await
        .unwrap();
    let id = FieldUuid(Uuid::new_v4());
    let row = ImplicitUuid {
        id: id.clone(),
        title: FieldVarchar::new("hello".to_string()).unwrap(),
    };
    insert(&pool, row).await.unwrap();
    let got = get::<_, ImplicitUuid>(&pool, &id.to_sql_value())
        .await
        .unwrap()
        .expect("row not found");
    assert_eq!(got.title.as_str(), "hello");
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_implicit_pk_bigint() {
    let pool = pool().await;
    sqlx::query(
        r#"CREATE TABLE "implicit_big" ("id" INTEGER PRIMARY KEY AUTOINCREMENT, "name" TEXT NOT NULL)"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    let row = ImplicitBig {
        id: FieldAutoBigInt(None),
        name: FieldText("x".to_string()),
    };
    let saved = insert(&pool, row).await.unwrap();
    assert_eq!(saved.id.0, Some(1));
}

// `#[model(...)]` is a derive helper: it must keep working next to the `model` macro.
mod clash_prelude {
    use rango::prelude::*;

    #[derive(Model)]
    #[model(table = "a", ordering = [asc("name")])]
    pub struct A {
        id: FieldUuid,
        name: FieldText,
    }
}

mod clash_glob {
    use rango::*;

    #[derive(Model)]
    #[model(table = "b", ordering = [asc("name")])]
    pub struct B {
        id: FieldUuid,
        name: FieldText,
    }
}

mod clash_attr_and_helper {
    use rango::prelude::*;

    #[rango::model]
    #[model(table = "c", ordering = [asc("name")])]
    pub struct C {
        name: FieldText,
    }

    #[test]
    fn schema_has_implicit_id() {
        let s = C::schema();
        assert_eq!(s.table_name, "c");
        assert_eq!(s.columns[0].name, "id");
        assert!(s.columns[0].primary_key);
        assert_eq!(s.ordering.len(), 1);
    }
}

// ── Partial selects ───────────────────────────────────────────────────────────

#[derive(Model, Debug, Clone)]
#[model(table = "rango_test_partial_person")]
struct TestPartialPerson {
    id: FieldUuid,
    #[field(column = "full_name")]
    name: FieldVarchar<1, 100>,
    age: FieldInt,
    nick: Option<FieldText>,
}

#[derive(rango_orm_derive::Partial, Debug, PartialEq)]
#[partial(model = TestPartialPerson)]
struct PersonLite {
    name: FieldVarchar<1, 100>,
    nick: Option<FieldText>,
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_select_as_partial() {
    let pool = pool().await;
    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_partial_person""#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(r#"CREATE TABLE "rango_test_partial_person" ("id" TEXT PRIMARY KEY, "full_name" TEXT NOT NULL, "age" INTEGER NOT NULL, "nick" TEXT)"#).execute(&pool).await.unwrap();

    for (name, age, nick) in [
        ("Cleo", 40, None),
        ("Ann", 30, Some("annie")),
        ("Bob", 30, None),
    ] {
        let person = TestPartialPerson {
            id: FieldUuid(Uuid::new_v4()),
            name: FieldVarchar::new(name.to_string()).unwrap(),
            age: FieldInt(age),
            nick: nick.map(|n| FieldText(n.to_string())),
        };
        insert(&pool, person).await.unwrap();
    }

    // Filter + order; the renamed `full_name` column and the NULL nick both decode.
    let rows: Vec<PersonLite> = TestPartialPerson::filter(&pool)
        .eq(TestPartialPerson::AGE, 30)
        .order_by(TestPartialPerson::NAME.asc())
        .select_as::<PersonLite>()
        .all()
        .await
        .unwrap();
    assert_eq!(
        rows,
        vec![
            PersonLite {
                name: FieldVarchar::new("Ann".to_string()).unwrap(),
                nick: Some(FieldText("annie".to_string())),
            },
            PersonLite {
                name: FieldVarchar::new("Bob".to_string()).unwrap(),
                nick: None,
            },
        ]
    );

    let one = TestPartialPerson::filter(&pool)
        .order_by(TestPartialPerson::AGE.desc())
        .select_as::<PersonLite>()
        .one()
        .await
        .unwrap();
    assert_eq!(
        one.unwrap().name,
        FieldVarchar::new("Cleo".to_string()).unwrap()
    );

    let none = TestPartialPerson::filter(&pool)
        .eq(TestPartialPerson::AGE, 99)
        .select_as::<PersonLite>()
        .one()
        .await
        .unwrap();
    assert!(none.is_none());
}

/// Minimal subscriber: records `(span name, "field=value" pairs)` for every new span.
#[derive(Clone, Default)]
#[allow(clippy::type_complexity)]
struct SpanLog(std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<String>)>>>);

impl tracing::Subscriber for SpanLog {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attrs: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        struct V(Vec<String>);
        impl tracing::field::Visit for V {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                self.0.push(format!("{}={:?}", f.name(), v));
            }
        }
        let mut v = V(vec![]);
        attrs.record(&mut v);
        let mut log = self.0.lock().unwrap();
        log.push((attrs.metadata().name().to_string(), v.0));
        tracing::span::Id::from_u64(log.len() as u64)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, ev: &tracing::Event<'_>) {
        struct V(Vec<String>);
        impl tracing::field::Visit for V {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                self.0.push(format!("{}={:?}", f.name(), v));
            }
        }
        let mut v = V(vec![]);
        ev.record(&mut v);
        self.0
            .lock()
            .unwrap()
            .push((format!("event:{}", ev.metadata().level()), v.0));
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_select_emits_db_query_span() {
    let pool = pool().await;
    setup(&pool).await;
    let log = SpanLog::default();
    let _guard = tracing::subscriber::set_default(log.clone());
    TestUser::filter(&pool).all().await.unwrap();
    let spans = log.0.lock().unwrap();
    let (_, fields) = spans
        .iter()
        .find(|(name, _)| name == "db.query")
        .expect("no db.query span");
    assert!(fields.contains(&"db.sql.table=\"test_user\"".to_string()));
    assert!(fields.contains(&"db.operation=\"SELECT\"".to_string()));
    assert!(fields.contains(&"db.system=\"sqlite\"".to_string()));
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_keyset_pagination_after_and_before() {
    let pool = pool().await;
    setup(&pool).await;

    let users: Vec<TestUser> = (0..25)
        .map(|i| user(&format!("page{i}@test.com")))
        .collect();
    bulk_create(&pool, &users).await.unwrap();
    let mut ids: Vec<Uuid> = users.iter().map(|u| u.id.0).collect();
    ids.sort();

    // Forward: pages of 10 with `after(last id)`.
    let mut seen = Vec::new();
    let mut last: Option<Uuid> = None;
    loop {
        let mut q = TestUser::filter(&pool).limit(10);
        if let Some(id) = last {
            q = q.after(TestUser::ID, id);
        }
        let page = q.order_by(TestUser::ID).all().await.unwrap();
        let Some(row) = page.last() else { break };
        last = Some(row.id.0);
        seen.extend(page.iter().map(|r| r.id.0));
    }
    assert_eq!(seen, ids);

    // Backward: mirror with `before`, newest first.
    let mut seen = Vec::new();
    let mut first: Option<Uuid> = None;
    loop {
        let mut q = TestUser::filter(&pool).limit(10);
        if let Some(id) = first {
            q = q.before(TestUser::ID, id);
        }
        let page = q.order_by(TestUser::ID.desc()).all().await.unwrap();
        let Some(row) = page.last() else { break };
        first = Some(row.id.0);
        seen.extend(page.iter().map(|r| r.id.0));
    }
    ids.reverse();
    assert_eq!(seen, ids);
}

// ── Model hooks ───────────────────────────────────────────────────────────────

static HOOK_LOG: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn hook_log(event: &str, name: &str) {
    HOOK_LOG.lock().unwrap().push(format!("{event}:{name}"));
}

fn hook_events(name: &str) -> Vec<String> {
    let suffix = format!(":{name}");
    let log = HOOK_LOG.lock().unwrap();
    log.iter()
        .filter(|e| e.ends_with(&suffix))
        .map(|e| e.trim_end_matches(&suffix).to_string())
        .collect()
}

#[derive(Model, Debug, Clone)]
#[model(table = "test_hooked", hooks)]
struct Hooked {
    id: FieldUuid,
    name: FieldVarchar<1, 100>,
    slug: FieldVarchar<0, 100>,
}

impl ModelHooks for Hooked {
    fn before_save(&mut self) -> BoxFuture<'_> {
        Box::pin(async move {
            self.slug = FieldVarchar::new(self.name.as_str().to_lowercase()).unwrap();
            hook_log("before_save", self.name.as_str());
            Ok(())
        })
    }
    fn after_save(&self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("after_save", self.name.as_str());
            if self.name.as_str().starts_with("fail_after") {
                return Err("after_save refused".into());
            }
            Ok(())
        })
    }
    fn before_create(&mut self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("before_create", self.name.as_str());
            if self.name.as_str().starts_with("fail_create") {
                return Err("before_create refused".into());
            }
            Ok(())
        })
    }
    fn after_create(&self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("after_create", self.name.as_str());
            Ok(())
        })
    }
    fn before_update(&mut self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("before_update", self.name.as_str());
            Ok(())
        })
    }
    fn after_update(&self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("after_update", self.name.as_str());
            Ok(())
        })
    }
    fn before_delete(&self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("before_delete", self.name.as_str());
            Ok(())
        })
    }
    fn after_delete(&self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("after_delete", self.name.as_str());
            Ok(())
        })
    }
}

fn hooked(name: &str) -> Hooked {
    Hooked {
        id: FieldUuid(Uuid::new_v4()),
        name: FieldVarchar::new(name.to_string()).unwrap(),
        slug: FieldVarchar::new(String::new()).unwrap(),
    }
}

async fn hooked_setup(pool: &SqlitePool) {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS "test_hooked" (
            "id"   TEXT PRIMARY KEY,
            "name" VARCHAR(100) NOT NULL UNIQUE,
            "slug" VARCHAR(100) NOT NULL
        )"#,
    )
    .execute(pool)
    .await
    .expect("setup: create hooked table");
}

async fn hooked_count(pool: &SqlitePool, name: &str) -> usize {
    Hooked::filter(pool)
        .eq("name", name)
        .all()
        .await
        .unwrap()
        .len()
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_hooks_insert_update_delete() {
    let pool = pool().await;
    hooked_setup(&pool).await;

    let saved = insert(&pool, hooked("Ada Lovelace")).await.unwrap();
    // before_save mutated the model and the mutation is what was written
    assert_eq!(saved.slug.as_str(), "ada lovelace");
    let stored = get::<_, Hooked>(&pool, &saved.id.to_sql_value())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.slug.as_str(), "ada lovelace");
    assert_eq!(
        hook_events("Ada Lovelace"),
        ["before_save", "before_create", "after_create", "after_save"]
    );

    // update: save + update hooks, never create hooks
    let mut changed = saved.clone();
    changed.slug = FieldVarchar::new("tampered").unwrap();
    let updated = update(&pool, changed).await.unwrap();
    assert_eq!(updated.slug.as_str(), "ada lovelace");
    assert_eq!(
        hook_events("Ada Lovelace")[4..],
        ["before_save", "before_update", "after_update", "after_save"]
    );

    delete(&pool, &updated).await.unwrap();
    assert_eq!(hooked_count(&pool, "Ada Lovelace").await, 0);
    assert_eq!(
        hook_events("Ada Lovelace")[8..],
        ["before_delete", "after_delete"]
    );
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_hooks_before_error_cancels_insert() {
    let pool = pool().await;
    hooked_setup(&pool).await;

    let err = insert(&pool, hooked("fail_create_1")).await.unwrap_err();
    assert!(matches!(err, Error::Hook(_)), "got {err:?}");
    assert_eq!(err.to_string(), "hook failed: before_create refused");
    assert_eq!(hooked_count(&pool, "fail_create_1").await, 0);
    assert_eq!(
        hook_events("fail_create_1"),
        ["before_save", "before_create"]
    );
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_hooks_after_error_rolls_back_in_transaction() {
    let pool = pool().await;
    hooked_setup(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let err = insert(&mut *tx, hooked("fail_after_tx")).await.unwrap_err();
    assert!(matches!(err, Error::Hook(_)), "got {err:?}");
    tx.rollback().await.unwrap();
    assert_eq!(hooked_count(&pool, "fail_after_tx").await, 0);

    // With a plain pool the statement is already committed.
    let err = insert(&pool, hooked("fail_after_pool")).await.unwrap_err();
    assert!(matches!(err, Error::Hook(_)), "got {err:?}");
    assert_eq!(hooked_count(&pool, "fail_after_pool").await, 1);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_hooks_get_or_create_only_on_insert() {
    let pool = pool().await;
    hooked_setup(&pool).await;

    let lookup = vec![("name", SqlValue::Text("goc_hooked".to_string()))];
    let (_, created) = get_or_create(&pool, lookup.clone(), hooked("goc_hooked"))
        .await
        .unwrap();
    assert!(created);
    let after_first = hook_events("goc_hooked");
    assert!(after_first.contains(&"after_create".to_string()));

    let (_, created) = get_or_create(&pool, lookup, hooked("goc_hooked"))
        .await
        .unwrap();
    assert!(!created);
    let after_second = hook_events("goc_hooked");
    // second call: before_* ran on the defaults, after_* did not
    assert_eq!(after_second.len(), after_first.len() + 2);
    assert_eq!(
        after_second[after_first.len()..],
        ["before_save", "before_create"]
    );
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_model_without_hooks_attribute_still_works() {
    let pool = pool().await;
    setup(&pool).await;
    let saved = insert(&pool, user("nohooks@hooks.com")).await.unwrap();
    delete(&pool, &saved).await.unwrap();
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_in_values_empty_list() {
    let pool = pool().await;
    setup(&pool).await;
    insert(&pool, user("in1@t.com")).await.unwrap();
    insert(&pool, user("in2@t.com")).await.unwrap();

    let none = TestUser::filter(&pool)
        .in_values(TestUser::EMAIL, vec![])
        .all()
        .await
        .unwrap();
    assert!(none.is_empty());

    let all = TestUser::filter(&pool)
        .not()
        .in_values(TestUser::EMAIL, vec![])
        .all()
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_all_warns_when_default_cap_is_hit() {
    let pool = pool().await;
    setup(&pool).await;
    let users: Vec<TestUser> = (0..1001)
        .map(|i| user(&format!("cap{i}@test.com")))
        .collect();
    bulk_create(&pool, &users).await.unwrap();

    let warned = |log: &SpanLog| {
        log.0
            .lock()
            .unwrap()
            .iter()
            .any(|(n, f)| n == "event:WARN" && f.iter().any(|x| x.contains("test_user")))
    };
    let log = SpanLog::default();
    {
        let _guard = tracing::subscriber::set_default(log.clone());
        let rows = TestUser::filter(&pool).all().await.unwrap();
        assert_eq!(rows.len(), 1000);
    }
    assert!(warned(&log));

    // Explicit cap or unlimited: no warning.
    let log2 = SpanLog::default();
    {
        let _guard = tracing::subscriber::set_default(log2.clone());
        assert_eq!(
            TestUser::filter(&pool)
                .unlimited()
                .all()
                .await
                .unwrap()
                .len(),
            1001
        );
        assert_eq!(
            TestUser::filter(&pool)
                .limit(1000)
                .all()
                .await
                .unwrap()
                .len(),
            1000
        );
    }
    assert!(!warned(&log2));
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_keyset_after_does_not_duplicate_order_by() {
    let pool = pool().await;
    setup(&pool).await;
    let sql = TestUser::filter(&pool)
        .order_by(TestUser::ID)
        .after(TestUser::ID, FieldUuid(uuid::Uuid::new_v4()))
        .explain();
    assert_eq!(sql.matches("ORDER BY").count(), 1, "{sql}");
    let order = sql.split("ORDER BY").nth(1).unwrap();
    assert_eq!(order.matches("id").count(), 1, "{sql}");
}

// ── Transactions everywhere: filter + M2M + bulk in one rolled-back atomic ───────

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_filter_m2m_bulk_roll_back_in_atomic() {
    let pool = pool().await;
    setup(&pool).await;

    for ddl in [
        r#"CREATE TABLE "test_blog" ("id" TEXT PRIMARY KEY, "title" TEXT NOT NULL)"#,
        r#"CREATE TABLE "test_tag" ("id" TEXT PRIMARY KEY, "name" TEXT NOT NULL)"#,
        r#"CREATE TABLE "test_blog_test_tag" ("test_blog_id" TEXT NOT NULL, "test_tag_id" TEXT NOT NULL, PRIMARY KEY ("test_blog_id", "test_tag_id"))"#,
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }

    let blog = TestBlog {
        id: FieldUuid(Uuid::new_v4()),
        title: FieldVarchar::new("tx".to_string()).unwrap(),
        tags: ManyToMany::empty(),
    };
    let tag = TestTag {
        id: FieldUuid(Uuid::new_v4()),
        name: FieldVarchar::new("tx".to_string()).unwrap(),
    };
    insert(&pool, blog.clone()).await.unwrap();
    insert(&pool, tag.clone()).await.unwrap();
    insert(&pool, user("keep@tx.com")).await.unwrap();

    let (b, t) = (blog.clone(), tag.clone());
    let err = atomic(&pool, async |tx| {
        bulk_create(&mut **tx, &[user("a@tx.com"), user("b@tx.com")]).await?;
        // Reads see the transaction's own writes.
        assert_eq!(TestUser::filter(&mut **tx).count().await?, 3);
        M2M::<TestBlog, TestTag>::add(&mut **tx, &b, &t).await?;
        assert!(M2M::<TestBlog, TestTag>::exists(&mut **tx, &b, &t).await?);
        M2M::<TestBlog, TestTag>::set(&mut **tx, &b, &[]).await?;
        M2M::<TestBlog, TestTag>::add(&mut **tx, &b, &t).await?;
        Err::<(), Error>(Error::abort("roll back"))
    })
    .await
    .unwrap_err();
    assert!(matches!(err, Error::Aborted(_)), "{err:?}");

    assert_eq!(TestUser::filter(&pool).count().await.unwrap(), 1);
    assert!(
        !M2M::<TestBlog, TestTag>::exists(&pool, &blog, &tag)
            .await
            .unwrap()
    );

    // Same calls commit with the transaction.
    atomic(&pool, async |tx| {
        bulk_create(&mut **tx, &[user("c@tx.com")]).await?;
        M2M::<TestBlog, TestTag>::add(&mut **tx, &blog, &tag).await?;
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(TestUser::filter(&pool).count().await.unwrap(), 2);
}

// ── bulk_update / bulk_upsert / update_or_create ─────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_bulk_update_bulk_upsert_update_or_create() {
    let pool = pool().await;
    setup(&pool).await;

    let mut users: Vec<TestUser> = (0..3).map(|i| user(&format!("bu{i}@t.com"))).collect();
    bulk_create(&pool, &users).await.unwrap();
    for (i, u) in users.iter_mut().enumerate() {
        u.name = FieldVarchar::new(format!("renamed{i}")).unwrap();
        u.active = FieldBool(false);
    }
    assert_eq!(
        bulk_update(&pool, &users, &["name", "active"])
            .await
            .unwrap(),
        3
    );
    let stored = get::<_, TestUser>(&pool, &users[1].id.to_sql_value())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.name.as_str(), "renamed1");
    assert!(!stored.active.0);
    // Unknown column and PK are rejected before any SQL runs.
    for bad in ["nope", "id"] {
        let err = bulk_update(&pool, &users, &[bad]).await.unwrap_err();
        assert!(matches!(err, Error::Invalid(_)), "{err:?}");
    }

    // Upsert: existing email keeps its id and takes the new name, new email is inserted.
    let mut again = user("bu0@t.com");
    again.name = FieldVarchar::new("upserted".to_string()).unwrap();
    bulk_upsert(&pool, &[again, user("bu9@t.com")], &["email"])
        .await
        .unwrap();
    assert_eq!(TestUser::filter(&pool).count().await.unwrap(), 4);
    let row = TestUser::filter(&pool)
        .eq("email", "bu0@t.com")
        .all()
        .await
        .unwrap();
    assert_eq!(row[0].id.0, users[0].id.0);
    assert_eq!(row[0].name.as_str(), "upserted");

    // update_or_create: creates, then updates the row it finds (same PK).
    let lookup = vec![("email", SqlValue::Text("uoc@t.com".to_string()))];
    let mut first = user("uoc@t.com");
    first.name = FieldVarchar::new("first".to_string()).unwrap();
    let created = update_or_create(&pool, lookup.clone(), first)
        .await
        .unwrap();
    assert_eq!(created.name.as_str(), "first");
    let mut second = user("uoc@t.com");
    second.name = FieldVarchar::new("second".to_string()).unwrap();
    let updated = update_or_create(&pool, lookup, second).await.unwrap();
    assert_eq!(updated.id.0, created.id.0);
    assert_eq!(updated.name.as_str(), "second");
    assert_eq!(
        TestUser::filter(&pool)
            .eq("email", "uoc@t.com")
            .count()
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_hooks_update_or_create() {
    let pool = pool().await;
    hooked_setup(&pool).await;

    let lookup = vec![("name", SqlValue::Text("uoc_hooked".to_string()))];
    update_or_create(&pool, lookup.clone(), hooked("uoc_hooked"))
        .await
        .unwrap();
    assert_eq!(
        hook_events("uoc_hooked"),
        ["before_save", "before_create", "after_create", "after_save"]
    );
    update_or_create(&pool, lookup, hooked("uoc_hooked"))
        .await
        .unwrap();
    assert_eq!(
        hook_events("uoc_hooked")[4..],
        ["before_save", "before_update", "after_update", "after_save"]
    );
}

// ── atomic: the future is Send (usable from a multi-thread spawn) ────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_atomic_future_is_send() {
    let pool = pool().await;
    setup(&pool).await;

    let p = pool.clone();
    let n = tokio::spawn(async move {
        atomic(&p, async |tx| {
            insert(&mut **tx, user("spawned@tx.com")).await?;
            TestUser::filter(&mut **tx).count().await
        })
        .await
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(n, 1);
}

// ── Foreign keys and M2M follow the target's primary key type ─────────────────

#[derive(Model, Debug, Clone)]
#[model(table = "pk_author")]
struct PkAuthor {
    id: FieldAutoBigInt,
    name: FieldVarchar<1, 100>,
}

#[derive(Model, Debug, Clone)]
#[model(table = "pk_label")]
struct PkLabel {
    id: FieldAutoInt,
    name: FieldVarchar<1, 100>,
}

#[derive(Model, Debug, Clone)]
#[model(table = "pk_book")]
struct PkBook {
    id: FieldUuid,
    author_id: ForeignKey<PkAuthor>,
    editor_id: Option<ForeignKey<PkLabel>>,
    title: FieldVarchar<1, 100>,
    labels: ManyToMany<PkLabel>,
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn sqlite_test_fk_and_m2m_follow_the_target_pk_type() {
    let pool = pool().await;
    for ddl in [
        r#"CREATE TABLE "pk_author" ("id" INTEGER PRIMARY KEY AUTOINCREMENT, "name" TEXT NOT NULL)"#,
        r#"CREATE TABLE "pk_label" ("id" INTEGER PRIMARY KEY AUTOINCREMENT, "name" TEXT NOT NULL)"#,
        r#"CREATE TABLE "pk_book" ("id" TEXT PRIMARY KEY, "author_id" INTEGER NOT NULL REFERENCES "pk_author" ("id"), "editor_id" INTEGER REFERENCES "pk_label" ("id"), "title" TEXT NOT NULL)"#,
        r#"CREATE TABLE "pk_book_pk_label" ("pk_book_id" TEXT NOT NULL, "pk_label_id" INTEGER NOT NULL, PRIMARY KEY ("pk_book_id", "pk_label_id"))"#,
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }

    // The schema derived from the models: FK columns take the target's key type.
    let col = |name: &str| {
        PkBook::schema()
            .columns
            .into_iter()
            .find(|c| c.name == name)
            .unwrap()
    };
    assert_eq!(col("author_id").col_type, ColumnType::BigInt);
    assert_eq!(col("editor_id").col_type, ColumnType::Int);
    assert_eq!(col("author_id").references.unwrap().table, "pk_author");

    let name = |s: &str| FieldVarchar::<1, 100>::new(s.to_string()).unwrap();
    let ann = insert(
        &pool,
        PkAuthor {
            id: FieldAutoBigInt(None),
            name: name("ann"),
        },
    )
    .await
    .unwrap();
    let bob = insert(
        &pool,
        PkAuthor {
            id: FieldAutoBigInt(None),
            name: name("bob"),
        },
    )
    .await
    .unwrap();
    let mut labels = Vec::new();
    for n in ["rust", "orm"] {
        let label = insert(
            &pool,
            PkLabel {
                id: FieldAutoInt(None),
                name: name(n),
            },
        )
        .await
        .unwrap();
        labels.push(label);
    }
    let (ann_id, bob_id) = (ann.id.0.unwrap(), bob.id.0.unwrap());
    let rust_id: i32 = labels[0].id.0.unwrap();

    // Insert and read back: bigint FK, nullable int FK (Some and None).
    let book = |title: &str, author: i64, editor: Option<i32>| PkBook {
        id: FieldUuid(Uuid::new_v4()),
        author_id: ForeignKey::new(author),
        editor_id: editor.map(ForeignKey::new),
        title: name(title),
        labels: ManyToMany::empty(),
    };
    let b1 = insert(&pool, book("one", ann_id, Some(rust_id)))
        .await
        .unwrap();
    let b2 = insert(&pool, book("two", ann_id, None)).await.unwrap();
    insert(&pool, book("three", bob_id, None)).await.unwrap();
    let got = get::<_, PkBook>(&pool, &b1.id.to_sql_value())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.author_id.id(), ann_id);
    assert_eq!(got.editor_id.map(|e| e.id()), Some(rust_id));
    let got2 = get::<_, PkBook>(&pool, &b2.id.to_sql_value())
        .await
        .unwrap()
        .unwrap();
    assert!(got2.editor_id.is_none());

    // Filter by the raw key value.
    let anns = PkBook::filter(&pool)
        .eq(PkBook::AUTHOR_ID, ann_id)
        .all()
        .await
        .unwrap();
    assert_eq!(anns.len(), 2);

    // select_related: one JOIN, the author's bigint key decodes.
    let joined = PkBook::filter(&pool)
        .select_related(PkBook::AUTHOR_ID)
        .order_by("title")
        .all()
        .await
        .unwrap();
    assert_eq!(joined.len(), 3);
    // titles sorted: one, three, two
    assert_eq!(joined[0].related::<PkAuthor>().name.as_str(), "ann");
    assert_eq!(joined[1].related::<PkAuthor>().id.0, Some(bob_id));

    // prefetch_related over the reverse bigint FK.
    let authors = PkAuthor::filter(&pool)
        .prefetch_related(PkBook::AUTHOR_ID)
        .all()
        .await
        .unwrap();
    let books_of = |id: i64| {
        authors
            .iter()
            .find(|a| a.id.0 == Some(id))
            .unwrap()
            .prefetched::<PkBook>()
            .len()
    };
    assert_eq!((books_of(ann_id), books_of(bob_id)), (2, 1));

    // ManyToMany to an int key: add / all / reverse / exists / prefetch / remove.
    for l in &labels {
        M2M::<PkBook, PkLabel>::add(&pool, &b1, l).await.unwrap();
    }
    M2M::<PkBook, PkLabel>::add(&pool, &b2, &labels[0])
        .await
        .unwrap();
    assert_eq!(
        M2M::<PkBook, PkLabel>::all(&pool, &b1).await.unwrap().len(),
        2
    );
    assert_eq!(
        M2M::<PkBook, PkLabel>::reverse(&pool, &labels[0])
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(
        M2M::<PkBook, PkLabel>::exists(&pool, &b2, &labels[0])
            .await
            .unwrap()
    );
    assert!(
        !M2M::<PkBook, PkLabel>::exists(&pool, &b2, &labels[1])
            .await
            .unwrap()
    );
    let with_labels = PkBook::filter(&pool)
        .prefetch_related(PkBook::LABELS)
        .all()
        .await
        .unwrap();
    let labels_of = |b: &PkBook| {
        with_labels
            .iter()
            .find(|w| w.id == b.id)
            .unwrap()
            .prefetched::<PkLabel>()
            .len()
    };
    assert_eq!((labels_of(&b1), labels_of(&b2)), (2, 1));
    M2M::<PkBook, PkLabel>::remove(&pool, &b1, &labels[1])
        .await
        .unwrap();
    assert_eq!(
        M2M::<PkBook, PkLabel>::all(&pool, &b1).await.unwrap().len(),
        1
    );
}

// ── bulk_update: one statement per chunk, mixed types ─────────────────────────────

#[derive(Model, Debug, Clone, PartialEq)]
#[model(table = "bulk_mix")]
struct BulkMix {
    id: FieldUuid,
    flag: FieldBool,
    n: Option<FieldInt>,
    big: FieldBigInt,
    price: FieldDecimal<10, 2>,
    at: FieldDateTime,
    nick: Option<FieldText>,
    meta: FieldJson,
}

#[derive(Model, Debug, Clone, PartialEq)]
#[model(table = "bulk_counter")]
struct BulkCounter {
    id: FieldAutoInt,
    label: FieldText,
}

fn bulk_mix(i: i64) -> BulkMix {
    BulkMix {
        id: FieldUuid(Uuid::new_v4()),
        flag: FieldBool(i % 2 == 0),
        n: if i % 3 == 0 {
            None
        } else {
            Some(FieldInt(i as i32))
        },
        big: FieldBigInt(i * 10_000_000_000),
        price: FieldDecimal::from_decimal(Decimal::new(i * 125, 2)),
        at: FieldDateTime(chrono::DateTime::from_timestamp(1_700_000_000 + i, 0).unwrap()),
        nick: if i % 5 == 0 {
            None
        } else {
            Some(FieldText(format!("nick \"{i}\" 'q'")))
        },
        meta: FieldJson(serde_json::json!({"i": i, "tags": ["a", "b"], "nested": {"ok": true}})),
    }
}

async fn bulk_mix_sorted(pool: &SqlitePool) -> Vec<BulkMix> {
    let mut rows: Vec<BulkMix> = BulkMix::filter(pool)
        .unlimited()
        .all()
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.inner)
        .collect();
    rows.sort_by_key(|r| r.id.0);
    rows
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_bulk_update_mixed_types_over_several_chunks() {
    let pool = pool().await;
    for ddl in [
        r#"DROP TABLE IF EXISTS "bulk_mix""#,
        r#"DROP TABLE IF EXISTS "bulk_counter""#,
        r#"CREATE TABLE "bulk_mix" ("id" TEXT PRIMARY KEY, "flag" INTEGER NOT NULL, "n" INTEGER, "big" INTEGER NOT NULL, "price" NUMERIC NOT NULL, "at" TEXT NOT NULL, "nick" TEXT, "meta" TEXT NOT NULL)"#,
        r#"CREATE TABLE "bulk_counter" ("id" INTEGER PRIMARY KEY, "label" TEXT NOT NULL)"#,
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }

    // Enough rows for at least two chunks of the multi-row UPDATE.
    let mut rows: Vec<BulkMix> = (0..3700).map(bulk_mix).collect();
    assert_eq!(bulk_create(&pool, &rows).await.unwrap(), 3700);

    // Every column changes, NULLs become values and values become NULL.
    for (i, r) in rows.iter_mut().enumerate() {
        let i = i as i64;
        r.flag = FieldBool(!r.flag.0);
        r.n = if r.n.is_some() {
            None
        } else {
            Some(FieldInt(-(i as i32)))
        };
        r.big = FieldBigInt(-i * 7_000_000_000);
        r.price = FieldDecimal::from_decimal(Decimal::new(i * 3 + 1, 2));
        r.at = FieldDateTime(chrono::DateTime::from_timestamp(1_600_000_000 + i * 60, 0).unwrap());
        r.nick = if r.nick.is_some() {
            None
        } else {
            Some(FieldText(format!("back {i}")))
        };
        r.meta = FieldJson(serde_json::json!({"changed": i, "s": "x\"y"}));
    }
    let t = std::time::Instant::now();
    let cols = ["flag", "n", "big", "price", "at", "nick", "meta"];
    assert_eq!(bulk_update(&pool, &rows, &cols).await.unwrap(), 3700);
    println!("sqlite bulk_update of 3700 rows: {:?}", t.elapsed());
    rows.sort_by_key(|r| r.id.0);
    assert_eq!(bulk_mix_sorted(&pool).await, rows);

    // Unchanged rows still count as matched; a subset of columns leaves the others alone.
    assert_eq!(
        bulk_update(&pool, &rows, &["n", "nick"]).await.unwrap(),
        3700
    );
    for r in rows.iter_mut() {
        r.n = Some(FieldInt(5));
        r.nick = None;
    }
    assert_eq!(
        bulk_update(&pool, &rows, &["n", "nick"]).await.unwrap(),
        3700
    );
    assert_eq!(bulk_mix_sorted(&pool).await, rows);

    // Inside atomic(): visible in the transaction, rolled back on error, kept on commit.
    for r in rows.iter_mut() {
        r.flag = FieldBool(!r.flag.0);
    }
    let before = bulk_mix_sorted(&pool).await;
    let snapshot = rows.clone();
    let res: Result<(), Error> = atomic(&pool, async |tx| {
        assert_eq!(bulk_update(&mut **tx, &snapshot, &["flag"]).await?, 3700);
        Err(Error::Invalid("rollback".into()))
    })
    .await;
    assert!(res.is_err());
    assert_eq!(bulk_mix_sorted(&pool).await, before);
    atomic(&pool, async |tx| {
        assert_eq!(bulk_update(&mut **tx, &snapshot, &["flag"]).await?, 3700);
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(bulk_mix_sorted(&pool).await, rows);

    // Rows not in the slice are untouched; unknown PKs match nothing.
    let mut some = rows[..10].to_vec();
    some[0].big = FieldBigInt(42);
    let mut ghost = bulk_mix(999_999);
    ghost.big = FieldBigInt(1);
    some.push(ghost);
    assert_eq!(bulk_update(&pool, &some, &["big"]).await.unwrap(), 10);
    rows[0].big = FieldBigInt(42);
    assert_eq!(bulk_mix_sorted(&pool).await, rows);

    // Auto-increment integer PK.
    let new = |l: String| BulkCounter {
        id: FieldAutoInt::default(),
        label: FieldText(l),
    };
    bulk_create(
        &pool,
        &(0..300).map(|i| new(format!("c{i}"))).collect::<Vec<_>>(),
    )
    .await
    .unwrap();
    let mut counters: Vec<BulkCounter> = BulkCounter::filter(&pool)
        .unlimited()
        .all()
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.inner)
        .collect();
    assert_eq!(counters.len(), 300);
    for c in counters.iter_mut() {
        c.label = FieldText(format!("renamed {}", c.id.0.unwrap()));
    }
    assert_eq!(
        bulk_update(&pool, &counters, &["label"]).await.unwrap(),
        300
    );
    let mut stored: Vec<BulkCounter> = BulkCounter::filter(&pool)
        .unlimited()
        .all()
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.inner)
        .collect();
    stored.sort_by_key(|c| c.id.0);
    counters.sort_by_key(|c| c.id.0);
    assert_eq!(stored, counters);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_decimal_filter_and_aggregates() {
    let pool = pool().await;
    for ddl in [
        r#"DROP TABLE IF EXISTS "bulk_mix""#,
        r#"CREATE TABLE "bulk_mix" ("id" TEXT PRIMARY KEY, "flag" INTEGER NOT NULL, "n" INTEGER, "big" INTEGER NOT NULL, "price" NUMERIC NOT NULL, "at" TEXT NOT NULL, "nick" TEXT, "meta" TEXT NOT NULL)"#,
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    // Prices 0.00, 1.25, 2.50, 3.75.
    let rows: Vec<BulkMix> = (0..4).map(bulk_mix).collect();
    bulk_create(&pool, &rows).await.unwrap();

    let at_least_two = BulkMix::filter(&pool)
        .gte(BulkMix::PRICE, Decimal::new(200, 2))
        .count()
        .await
        .unwrap();
    assert_eq!(at_least_two, 2);
    let exact = BulkMix::filter(&pool)
        .eq("price", Decimal::new(125, 2))
        .count()
        .await
        .unwrap();
    assert_eq!(exact, 1);
    let sum: Option<f64> = BulkMix::filter(&pool).sum("price").await.unwrap();
    assert_eq!(sum, Some(7.5));
    let avg: Option<f64> = BulkMix::filter(&pool).avg("price").await.unwrap();
    assert_eq!(avg, Some(1.875));
}

// ── Hostile input (docs/src/trust/security-requirements.md) ──────────────────

const HOSTILE: &str = "x'); DROP TABLE test_user; --";

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_hostile_values_are_bound() {
    let pool = pool().await;
    setup(&pool).await;
    let mut u = user("a@sqlite.com");
    u.name = FieldVarchar::new(HOSTILE.to_string()).unwrap();
    insert(&pool, u).await.unwrap();
    insert(&pool, user("b@sqlite.com")).await.unwrap();

    // The hostile text is matched as data, never run: exactly one row, table intact.
    let q = || TestUser::filter(&pool);
    assert_eq!(q().eq("name", HOSTILE).count().await.unwrap(), 1);
    assert_eq!(q().like("name", "x'%--").count().await.unwrap(), 1);
    assert_eq!(
        q().in_values("name", vec![SqlValue::from(HOSTILE), SqlValue::from("y")])
            .count()
            .await
            .unwrap(),
        1
    );
    assert_eq!(q().ne("name", HOSTILE).count().await.unwrap(), 1);
    assert_eq!(q().count().await.unwrap(), 2);

    // Backend extensions: path, value and FTS query are bound too.
    setup_article(&pool).await;
    insert(&pool, article_sqlite("A", r#"{"k":"v"}"#))
        .await
        .unwrap();
    let a = || TestArticleSqlite::filter(&pool);
    assert_eq!(
        a().json_extract_eq("metadata", "$.k", HOSTILE)
            .count()
            .await
            .unwrap(),
        0
    );
    // A hostile path or FTS query matches nothing (or errors): it is never extra SQL.
    let path = a().json_extract_exists("metadata", "$.k') OR 1=1 --");
    assert_eq!(path.count().await.unwrap_or(0), 0);
    let fts = a().fts5("test_article", HOSTILE);
    assert_eq!(fts.count().await.unwrap_or(0), 0);
    assert_eq!(q().count().await.unwrap(), 2);
}

#[derive(Model, Debug, Clone)]
#[model(table = "ev\"il; DROP TABLE test_user; --")]
struct EvilNames {
    id: FieldUuid,
    #[field(column = "na\"me); DROP TABLE test_user; --")]
    name: FieldVarchar<1, 100>,
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_hostile_identifiers_are_quoted() {
    let pool = pool().await;
    setup(&pool).await;
    let s = EvilNames::schema();
    let col = &s.columns[1].name;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE TABLE {} (\"id\" TEXT PRIMARY KEY, {} TEXT NOT NULL)",
        quote_ident(&s.table_name),
        quote_ident(col)
    )))
    .execute(&pool)
    .await
    .unwrap();

    let mut e = EvilNames {
        id: FieldUuid(Uuid::new_v4()),
        name: FieldVarchar::new("n".to_string()).unwrap(),
    };
    insert(&pool, e.clone()).await.unwrap();
    e.name = FieldVarchar::new("m".to_string()).unwrap();
    update(&pool, e.clone()).await.unwrap();
    let q = || EvilNames::filter(&pool);
    assert_eq!(
        q().eq(col.as_str(), "m")
            .order_by(col.as_str())
            .count()
            .await
            .unwrap(),
        1
    );
    // A string column name from input is checked against the schema, not quoted blindly.
    assert!(matches!(
        q().eq("name\" = 1 OR \"1", "m").count().await,
        Err(Error::Invalid(_))
    ));
    delete(&pool, &e).await.unwrap();
    assert_eq!(q().count().await.unwrap(), 0);
    assert_eq!(TestUser::filter(&pool).count().await.unwrap(), 0); // test_user still exists
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_bound_values_never_logged_or_in_errors() {
    let pool = pool().await;
    setup(&pool).await;
    let log = SpanLog::default();
    let _guard = tracing::subscriber::set_default(log.clone());
    let needle = "s3cr3t-needle";

    TestUser::filter(&pool)
        .eq("name", needle)
        .all()
        .await
        .unwrap();
    let missing = rango_orm_sqlite::raw_execute(
        &pool,
        "SELECT * FROM no_such_table WHERE name = ?",
        vec![SqlValue::from(needle)],
    )
    .await
    .unwrap_err();
    let bad_sql = rango_orm_sqlite::raw_execute(
        &pool,
        "SELEC nonsense WHERE name = ?",
        vec![SqlValue::from(needle)],
    )
    .await
    .unwrap_err();

    // Errors carry neither the bound value nor the statement text.
    for e in [&missing, &bad_sql] {
        let text = format!("{e} {e:?}");
        assert!(!text.contains(needle), "{text}");
        assert!(!text.contains("WHERE name"), "{text}");
    }
    // Rango's own spans never hold the value (sqlx's QueryLogger only stores the SQL text).
    let seen = format!("{:?}", log.0.lock().unwrap());
    assert!(seen.contains("db.query"), "no span recorded: {seen}");
    assert!(!seen.contains(needle), "{seen}");
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-sqlite"), ignore)]
async fn test_sqlite_unknown_lookup_and_conflict_columns_are_invalid() {
    let pool = pool().await;
    setup(&pool).await;
    let bogus = vec![("emial", SqlValue::Text("a@b.c".into()))];
    let err = get_or_create(&pool, bogus.clone(), user("a@b.c"))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
    let err = update_or_create(&pool, bogus, user("a@b.c"))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
    let err = bulk_upsert(&pool, &[user("a@b.c")], &["emial"])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}
