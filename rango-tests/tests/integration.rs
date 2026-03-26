/// Integration tests — require a real PostgreSQL database.
///
/// Set DATABASE_URL env var to run:
///   DATABASE_URL=postgres://postgres:postgres@localhost/rango_test cargo test --features integration
///
/// These tests create and drop their own tables using a unique prefix per run.

use rango_core::*;
use rango_derive::Model;
use rango_postgres::*;
use rango_postgres::PgQueryExt;
use uuid::Uuid;

// ── Test models ───────────────────────────────────────────────────────────────

#[derive(Model, Debug, Clone, PartialEq)]
#[model(table = "rango_test_user")]
struct TestUser {
    id: FieldUuid,
    #[field(unique)]
    email: FieldEmail,
    name: FieldVarchar<1, 100>,
    active: FieldBool,
    score: Option<FieldInt>,
}

#[derive(Model, Debug, Clone)]
#[model(table = "rango_test_post")]
struct TestPost {
    id: FieldUuid,
    author_id: ForeignKey<TestUser>,
    title: FieldVarchar<1, 255>,
    published: FieldBool,
}

#[derive(Model, Debug, Clone)]
#[model(table = "rango_test_article")]
struct TestArticle {
    id: FieldUuid,
    title: FieldVarchar<1, 255>,
    content: FieldText,
    metadata: FieldJson,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/rango_test".to_string());
    PgPool::connect(&url).await.expect("Failed to connect to test database")
}

async fn setup(pool: &PgPool) {
    // Create tables and wipe any leftover data from previous runs
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS "rango_test_user" (
            "id"     UUID PRIMARY KEY,
            "email"  VARCHAR(254) NOT NULL UNIQUE,
            "name"   VARCHAR(100) NOT NULL,
            "active" BOOLEAN NOT NULL,
            "score"  INTEGER
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("setup: create rango_test_user");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS "rango_test_post" (
            "id"        UUID PRIMARY KEY,
            "author_id" UUID NOT NULL,
            "title"     VARCHAR(255) NOT NULL,
            "published" BOOLEAN NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("setup: create rango_test_post");

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS "rango_test_article" (
            "id"       UUID PRIMARY KEY,
            "title"    VARCHAR(255) NOT NULL,
            "content"  TEXT NOT NULL,
            "metadata" JSONB NOT NULL DEFAULT '{}'
        )"#,
    )
    .execute(pool)
    .await
    .expect("setup: create rango_test_article");

    // Wipe data — tests are isolated by content, not by schema
    sqlx::query(r#"TRUNCATE "rango_test_post", "rango_test_user", "rango_test_article" RESTART IDENTITY CASCADE"#)
        .execute(pool)
        .await
        .expect("setup: truncate tables");
}

async fn teardown(pool: &PgPool) {
    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_article""#).execute(pool).await.ok();
    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_post""#).execute(pool).await.ok();
    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_user""#).execute(pool).await.ok();
}

fn article(title: &str, content: &str, metadata: serde_json::Value) -> TestArticle {
    TestArticle {
        id: FieldUuid(Uuid::new_v4()),
        title: FieldVarchar(title.to_string()),
        content: FieldText(content.to_string()),
        metadata: FieldJson(metadata),
    }
}

fn user(email: &str) -> TestUser {
    TestUser {
        id: FieldUuid(Uuid::new_v4()),
        email: FieldEmail(email.to_string()),
        name: FieldVarchar("Test User".to_string()),
        active: FieldBool(true),
        score: None,
    }
}

// ── CRUD ──────────────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_insert_and_get() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("insert@test.com");
    let inserted = insert(&pool, u.clone()).await.expect("insert failed");
    assert_eq!(inserted.email, u.email);

    let fetched = get::<_, TestUser>(&pool, &u.id.to_sql_value()).await
        .expect("get failed")
        .expect("user not found");
    assert_eq!(fetched.email, u.email);

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_update() {
    let pool = pool().await;
    setup(&pool).await;

    let mut u = user("update@test.com");
    insert(&pool, u.clone()).await.unwrap();

    u.name = FieldVarchar("Updated".to_string());
    let updated = update(&pool, u.clone()).await.expect("update failed");
    assert_eq!(updated.name, FieldVarchar("Updated".to_string()));

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_delete() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("delete@test.com");
    insert(&pool, u.clone()).await.unwrap();
    delete(&pool, &u).await.expect("delete failed");

    let fetched = get::<_, TestUser>(&pool, &u.id.to_sql_value()).await.unwrap();
    assert!(fetched.is_none());

    teardown(&pool).await;
}

// ── Query builder ─────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_filter_eq() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("active@test.com")).await.unwrap();
    let mut inactive = user("inactive@test.com");
    inactive.active = FieldBool(false);
    insert(&pool, inactive).await.unwrap();

    let results = TestUser::filter(&pool)
        .eq("active", true)
        .all()
        .await
        .expect("filter failed");

    assert!(results.iter().any(|u| u.email == FieldEmail("active@test.com".to_string())));
    assert!(!results.iter().any(|u| u.email == FieldEmail("inactive@test.com".to_string())));

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_count_and_exists() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("count1@test.com")).await.unwrap();
    insert(&pool, user("count2@test.com")).await.unwrap();

    let count = TestUser::filter(&pool).count().await.expect("count failed");
    assert!(count >= 2);

    let exists = TestUser::filter(&pool)
        .eq("email", "count1@test.com")
        .exists()
        .await
        .expect("exists failed");
    assert!(exists);

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_order_and_limit() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("z@test.com")).await.unwrap();
    insert(&pool, user("a@test.com")).await.unwrap();

    let results = TestUser::filter(&pool)
        .order_by("email")
        .limit(1)
        .all()
        .await
        .expect("order+limit failed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].email, FieldEmail("a@test.com".to_string()));

    teardown(&pool).await;
}

// ── Bulk ops ──────────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_bulk_create() {
    let pool = pool().await;
    setup(&pool).await;

    let users: Vec<TestUser> = (0..5)
        .map(|i| user(&format!("bulk{}@test.com", i)))
        .collect();

    let n = bulk_create(&pool, &users).await.expect("bulk_create failed");
    assert_eq!(n, 5);

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_bulk_update() {
    let pool = pool().await;
    setup(&pool).await;

    let mut users: Vec<TestUser> = (0..3)
        .map(|i| user(&format!("bupdate{}@test.com", i)))
        .collect();
    bulk_create(&pool, &users).await.unwrap();

    for u in &mut users {
        u.name = FieldVarchar("Updated".to_string());
    }
    let n = bulk_update(&pool, &users, &["name"]).await.expect("bulk_update failed");
    assert_eq!(n, 3);

    teardown(&pool).await;
}

// ── Transactions ──────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_atomic_commit() {
    let pool = pool().await;
    setup(&pool).await;

    // Use a transaction manually to avoid async closure lifetime issues
    let u = user("atomic@test.com");
    let mut tx = pool.begin().await.unwrap();
    insert(&mut *tx, u).await.expect("insert in tx failed");
    tx.commit().await.unwrap();

    let exists = TestUser::filter(&pool)
        .eq("email", "atomic@test.com")
        .exists()
        .await
        .unwrap();
    assert!(exists);

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_atomic_rollback() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("rollback@test.com");
    let mut tx = pool.begin().await.unwrap();
    insert(&mut *tx, u).await.expect("insert in tx failed");
    tx.rollback().await.unwrap(); // explicit rollback

    let exists = TestUser::filter(&pool)
        .eq("email", "rollback@test.com")
        .exists()
        .await
        .unwrap();
    assert!(!exists);

    teardown(&pool).await;
}

// ── get_or_create ─────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_get_or_create() {
    let pool = pool().await;
    setup(&pool).await;

    let defaults = user("goc@test.com");
    let lookup = vec![("email", SqlValue::Text("goc@test.com".to_string()))];

    let (u1, created1) = get_or_create(&pool, lookup.clone(), defaults.clone()).await.unwrap();
    assert!(created1);

    let (u2, created2) = get_or_create(&pool, lookup, defaults).await.unwrap();
    assert!(!created2);
    assert_eq!(u1.id, u2.id);

    teardown(&pool).await;
}

// ── Aggregations ──────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_aggregations() {
    let pool = pool().await;
    setup(&pool).await;

    let mut u1 = user("agg1@test.com");
    u1.score = Some(FieldInt(10));
    let mut u2 = user("agg2@test.com");
    u2.score = Some(FieldInt(20));
    let mut u3 = user("agg3@test.com");
    u3.score = Some(FieldInt(30));
    bulk_create(&pool, &[u1, u2, u3]).await.unwrap();

    let sum = TestUser::filter(&pool).sum("score").await.unwrap();
    assert_eq!(sum, Some(60.0));

    let avg = TestUser::filter(&pool).avg("score").await.unwrap();
    assert_eq!(avg, Some(20.0));

    let min = TestUser::filter(&pool).min::<i32>("score").await.unwrap();
    assert_eq!(min, Some(10));

    let max = TestUser::filter(&pool).max::<i32>("score").await.unwrap();
    assert_eq!(max, Some(30));

    teardown(&pool).await;
}

// ── Raw SQL ───────────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_raw_scalar() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("raw@test.com")).await.unwrap();

    let count: i64 = raw_scalar(
        &pool,
        "SELECT COUNT(*) FROM rango_test_user WHERE email = $1",
        vec![SqlValue::Text("raw@test.com".to_string())],
    )
    .await
    .expect("raw_scalar failed");

    assert_eq!(count, 1);

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_raw_execute() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("rawexec@test.com")).await.unwrap();

    let affected = raw_execute(
        &pool,
        "UPDATE rango_test_user SET active = false WHERE email = $1",
        vec![SqlValue::Text("rawexec@test.com".to_string())],
    )
    .await
    .expect("raw_execute failed");

    assert_eq!(affected, 1);

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_values_projection() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("values@test.com")).await.unwrap();

    let rows = TestUser::filter(&pool)
        .eq("email", "values@test.com")
        .values(&["email", "active"])
        .await
        .expect("values failed");

    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains_key("email"));
    assert!(rows[0].contains_key("active"));
    assert!(!rows[0].contains_key("name")); // not requested

    if let SqlValue::Text(email) = &rows[0]["email"] {
        assert_eq!(email, "values@test.com");
    } else {
        panic!("email should be SqlValue::Text");
    }

    teardown(&pool).await;
}

// ── PgQueryExt ────────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_pg_fts() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, article(
        "Rust ORM guide",
        "Rango is a Django-inspired ORM for Rust with async support",
        serde_json::json!({}),
    )).await.unwrap();
    insert(&pool, article(
        "Python tutorial",
        "Flask is a lightweight web framework for Python developers",
        serde_json::json!({}),
    )).await.unwrap();

    let results = TestArticle::filter(&pool)
        .fts("content", "Rust ORM async")
        .all()
        .await
        .expect("fts failed");

    assert_eq!(results.len(), 1);
    assert!(results[0].title.0.contains("Rust"));

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_pg_json_contains() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, article("Draft post", "content", serde_json::json!({"status": "draft"}))).await.unwrap();
    insert(&pool, article("Published post", "content", serde_json::json!({"status": "published"}))).await.unwrap();

    let results = TestArticle::filter(&pool)
        .json_contains("metadata", serde_json::json!({"status": "published"}))
        .all()
        .await
        .expect("json_contains failed");

    assert_eq!(results.len(), 1);
    assert!(results[0].title.0.contains("Published"));

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_pg_json_has_key() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, article("With views", "content", serde_json::json!({"views": 42}))).await.unwrap();
    insert(&pool, article("No views key", "content", serde_json::json!({"status": "draft"}))).await.unwrap();

    let results = TestArticle::filter(&pool)
        .json_has_key("metadata", "views")
        .all()
        .await
        .expect("json_has_key failed");

    assert_eq!(results.len(), 1);
    assert!(results[0].title.0.contains("With views"));

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_pg_json_field_eq() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, article("Active", "content", serde_json::json!({"status": "active"}))).await.unwrap();
    insert(&pool, article("Inactive", "content", serde_json::json!({"status": "inactive"}))).await.unwrap();

    let results = TestArticle::filter(&pool)
        .json_field_eq("metadata", "status", "active")
        .all()
        .await
        .expect("json_field_eq failed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title.0, "Active");

    teardown(&pool).await;
}
