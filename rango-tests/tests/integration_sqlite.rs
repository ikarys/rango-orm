/// SQLite integration tests — run fully in-memory, no Docker needed.
///
/// Run with:
///   cargo test --test integration_sqlite --features rango-tests/integration-sqlite
use rango_core::*;
use rango_derive::Model;
use rango_sqlite::*;
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

    u.name = FieldVarchar("Updated".to_string());
    let updated = update(&pool, u.clone()).await.expect("update failed");
    assert_eq!(updated.name, FieldVarchar("Updated".to_string()));
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
            .any(|u| u.email == FieldEmail("active@sqlite.com".to_string()))
    );
    assert!(
        !results
            .iter()
            .any(|u| u.email == FieldEmail("inactive@sqlite.com".to_string()))
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
                            rango_core::SqlValue::Text(s) => serde_json::Value::String(s.clone()),
                            rango_core::SqlValue::BigInt(n) => {
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
        Some(&rango_core::SqlValue::Text(
            "roundtrip@sqlite.com".to_string()
        ))
    );
    assert_eq!(
        rows[0].get("name"),
        Some(&rango_core::SqlValue::Text("Test User".to_string()))
    );
}

// ── SqliteQueryExt ────────────────────────────────────────────────────────────

use rango_sqlite::SqliteQueryExt;

#[derive(rango_derive::Model, Debug, Clone)]
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
        title: FieldVarchar(title.to_string()),
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
    assert_eq!(results[0].title.0, "Published");
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
    assert_eq!(results[0].title.0, "Rust ORM");
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
    assert_eq!(results[0].title.0, "With views");
}
