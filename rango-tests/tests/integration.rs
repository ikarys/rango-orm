/// Integration tests — require a real PostgreSQL database.
///
/// Set DATABASE_URL env var to run:
///   DATABASE_URL=postgres://postgres:postgres@localhost/rango_test cargo test --features integration
///
/// These tests create and drop their own tables using a unique prefix per run.
use rango_orm_core::*;
use rango_orm_derive::Model;
use rango_orm_postgres::PgQueryExt;
use rango_orm_postgres::*;
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

#[allow(dead_code)]
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

#[derive(Model, Debug, Clone)]
#[model(table = "rango_test_blog")]
struct TestBlog {
    id: FieldUuid,
    title: FieldVarchar<1, 100>,
    tags: ManyToMany<TestTag>,
}

#[derive(Model, Debug, Clone)]
#[model(table = "rango_test_tag")]
struct TestTag {
    id: FieldUuid,
    name: FieldVarchar<1, 100>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/rango_test".to_string());
    PgPool::connect(&url)
        .await
        .expect("Failed to connect to test database")
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
    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_article""#)
        .execute(pool)
        .await
        .ok();
    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_post""#)
        .execute(pool)
        .await
        .ok();
    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_user""#)
        .execute(pool)
        .await
        .ok();
}

fn article(title: &str, content: &str, metadata: serde_json::Value) -> TestArticle {
    TestArticle {
        id: FieldUuid(Uuid::new_v4()),
        title: FieldVarchar::new(title.to_string()).unwrap(),
        content: FieldText(content.to_string()),
        metadata: FieldJson(metadata),
    }
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_prefetch_related_many_to_many() {
    let pool = pool().await;

    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_blog_rango_test_tag""#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_blog""#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_tag""#)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query(
        r#"CREATE TABLE "rango_test_blog" (
            "id"    UUID PRIMARY KEY,
            "title" VARCHAR(100) NOT NULL
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"CREATE TABLE "rango_test_tag" (
            "id"   UUID PRIMARY KEY,
            "name" VARCHAR(100) NOT NULL
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"CREATE TABLE "rango_test_blog_rango_test_tag" (
            "rango_test_blog_id" UUID NOT NULL,
            "rango_test_tag_id"  UUID NOT NULL,
            PRIMARY KEY ("rango_test_blog_id", "rango_test_tag_id")
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

    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_blog_rango_test_tag""#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_blog""#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_tag""#)
        .execute(&pool)
        .await
        .unwrap();
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

// ── CRUD ──────────────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_insert_and_get() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("insert@test.com");
    let inserted = insert(&pool, u.clone()).await.expect("insert failed");
    assert_eq!(inserted.email, u.email);

    let fetched = get::<_, TestUser>(&pool, &u.id.to_sql_value())
        .await
        .expect("get failed")
        .expect("user not found");
    assert_eq!(fetched.email, u.email);

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_select_and_prefetch_related() {
    let pool = pool().await;
    setup(&pool).await;

    let alice = insert(&pool, user("alice@test.com")).await.unwrap();
    let bob = insert(&pool, user("bob@test.com")).await.unwrap();
    for title in ["first", "second"] {
        let post = TestPost {
            id: FieldUuid(Uuid::new_v4()),
            author_id: ForeignKey::new(alice.id.0),
            title: FieldVarchar::new(title.to_string()).unwrap(),
            published: FieldBool(true),
        };
        insert(&pool, post).await.unwrap();
    }

    let posts = TestPost::filter(&pool)
        .select_related::<TestUser>("author_id")
        .all()
        .await
        .unwrap();
    assert_eq!(posts.len(), 2);
    assert!(
        posts
            .iter()
            .all(|p| p.related::<TestUser>().email == alice.email)
    );

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

    // select_related is one LEFT JOIN; ORDER BY / LIMIT apply to posts.
    let q = TestPost::filter(&pool)
        .select_related(TestPost::AUTHOR_ID)
        .order_by("-title")
        .limit(1);
    assert!(q.explain().contains("LEFT JOIN"), "{}", q.explain());
    let plan = q.explain_analyze().await.unwrap();
    assert!(plan.contains("actual time"), "{plan}");
    let posts = q.all().await.unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].title.as_str(), "second");
    assert_eq!(posts[0].related::<TestUser>().email, alice.email);

    // EXISTS / NOT EXISTS on the reverse FK.
    let with_posts = TestUser::filter(&pool)
        .where_exists(TestPost::AUTHOR_ID, |q| q.eq(TestPost::TITLE, "first"))
        .all()
        .await
        .unwrap();
    assert_eq!(with_posts.len(), 1);
    assert_eq!(with_posts[0].email, alice.email);
    let without_posts = TestUser::filter(&pool)
        .not()
        .where_exists(TestPost::AUTHOR_ID, |q| q)
        .all()
        .await
        .unwrap();
    assert_eq!(without_posts.len(), 1);
    assert_eq!(without_posts[0].email, bob.email);

    // IN (subquery), combined with OR and a bound value before and after it.
    let posts = TestPost::filter(&pool)
        .eq("title", "nope".to_string())
        .or()
        .in_subquery(TestPost::AUTHOR_ID, TestUser::ID, |q| {
            q.eq(TestUser::EMAIL, alice.email.to_string())
        })
        .eq("published", true)
        .all()
        .await
        .unwrap();
    assert_eq!(posts.len(), 2);

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_update() {
    let pool = pool().await;
    setup(&pool).await;

    let mut u = user("update@test.com");
    insert(&pool, u.clone()).await.unwrap();

    u.name = FieldVarchar::new("Updated".to_string()).unwrap();
    let updated = update(&pool, u.clone()).await.expect("update failed");
    assert_eq!(
        updated.name,
        FieldVarchar::new("Updated".to_string()).unwrap()
    );

    teardown(&pool).await;
}

/// Regression: a non-text nullable column holding a value must not decode as NULL.
#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_option_non_text_roundtrip() {
    let pool = pool().await;
    setup(&pool).await;

    let mut u = user("option@test.com");
    u.score = Some(FieldInt(42));
    insert(&pool, u.clone()).await.unwrap();
    let none = user("none@test.com");
    insert(&pool, none.clone()).await.unwrap();

    let fetched = get::<_, TestUser>(&pool, &u.id.to_sql_value())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.score, Some(FieldInt(42)));
    let fetched = get::<_, TestUser>(&pool, &none.id.to_sql_value())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.score, None);

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

    let fetched = get::<_, TestUser>(&pool, &u.id.to_sql_value())
        .await
        .unwrap();
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

    assert!(
        results
            .iter()
            .any(|u| u.email == FieldEmail::new("active@test.com".to_string()).unwrap())
    );
    assert!(
        !results
            .iter()
            .any(|u| u.email == FieldEmail::new("inactive@test.com".to_string()).unwrap())
    );

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
    assert_eq!(
        results[0].email,
        FieldEmail::new("a@test.com".to_string()).unwrap()
    );

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

    let n = bulk_create(&pool, &users)
        .await
        .expect("bulk_create failed");
    assert_eq!(n, 5);

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_bulk_create_unnest_large_with_nulls() {
    let pool = pool().await;
    setup(&pool).await;

    // 1500 rows with a nullable column, all in one UNNEST statement.
    let mut users: Vec<TestUser> = (0..1500)
        .map(|i| user(&format!("unnest{}@test.com", i)))
        .collect();
    for (i, u) in users.iter_mut().enumerate() {
        u.score = (i % 3 != 0).then_some(FieldInt(i as i32));
    }

    let n = bulk_create(&pool, &users)
        .await
        .expect("bulk_create failed");
    assert_eq!(n, 1500);

    let mut fetched: Vec<TestUser> = TestUser::filter(&pool)
        .unlimited()
        .all()
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.inner)
        .collect();
    fetched.sort_by(|a, b| a.email.as_str().cmp(&b.email.as_str()));
    users.sort_by(|a, b| a.email.as_str().cmp(&b.email.as_str()));
    assert_eq!(fetched, users);

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
        u.name = FieldVarchar::new("Updated".to_string()).unwrap();
    }
    let n = bulk_update(&pool, &users, &["name"])
        .await
        .expect("bulk_update failed");
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

    let (u1, created1) = get_or_create(&pool, lookup.clone(), defaults.clone())
        .await
        .unwrap();
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

/// No `rango::call`: a procedure or function is plain SQL through `raw*`, values bound.
#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_stored_procedure_via_raw() {
    let pool = pool().await;
    raw_execute(
        &pool,
        "CREATE OR REPLACE PROCEDURE rango_test_double(IN x INT, INOUT result INT)
         LANGUAGE plpgsql AS $$ BEGIN result := x * 2; END $$",
        vec![],
    )
    .await
    .unwrap();

    // CALL returns the INOUT parameters as one row.
    let doubled: i32 = raw_scalar(
        &pool,
        "CALL rango_test_double($1, NULL)",
        vec![SqlValue::Int(21)],
    )
    .await
    .unwrap();
    assert_eq!(doubled, 42);

    raw_execute(&pool, "DROP PROCEDURE rango_test_double(INT, INT)", vec![])
        .await
        .unwrap();
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

    insert(
        &pool,
        article(
            "Rust ORM guide",
            "Rango is a Django-inspired ORM for Rust with async support",
            serde_json::json!({}),
        ),
    )
    .await
    .unwrap();
    insert(
        &pool,
        article(
            "Python tutorial",
            "Flask is a lightweight web framework for Python developers",
            serde_json::json!({}),
        ),
    )
    .await
    .unwrap();

    let results = TestArticle::filter(&pool)
        .fts("content", "Rust ORM async")
        .all()
        .await
        .expect("fts failed");

    assert_eq!(results.len(), 1);
    assert!(results[0].title.as_str().contains("Rust"));

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_pg_fts_lang_binds_language() {
    let pool = pool().await;
    setup(&pool).await;

    insert(
        &pool,
        article(
            "Guide",
            "Les chats mangent des souris",
            serde_json::json!({}),
        ),
    )
    .await
    .unwrap();

    // "chat" only matches "chats" through the French stemmer.
    let results = TestArticle::filter(&pool)
        .fts_lang("content", "chat", "french")
        .all()
        .await
        .expect("fts_lang failed");
    assert_eq!(results.len(), 1);

    // The language is a bound parameter: a quote is an unknown config, not SQL.
    let injected = TestArticle::filter(&pool)
        .fts_lang("content", "chat", "french') OR true --")
        .all()
        .await;
    assert!(injected.is_err());

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_pg_json_contains() {
    let pool = pool().await;
    setup(&pool).await;

    insert(
        &pool,
        article(
            "Draft post",
            "content",
            serde_json::json!({"status": "draft"}),
        ),
    )
    .await
    .unwrap();
    insert(
        &pool,
        article(
            "Published post",
            "content",
            serde_json::json!({"status": "published"}),
        ),
    )
    .await
    .unwrap();

    let results = TestArticle::filter(&pool)
        .json_contains("metadata", serde_json::json!({"status": "published"}))
        .all()
        .await
        .expect("json_contains failed");

    assert_eq!(results.len(), 1);
    assert!(results[0].title.as_str().contains("Published"));

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_pg_json_has_key() {
    let pool = pool().await;
    setup(&pool).await;

    insert(
        &pool,
        article("With views", "content", serde_json::json!({"views": 42})),
    )
    .await
    .unwrap();
    insert(
        &pool,
        article(
            "No views key",
            "content",
            serde_json::json!({"status": "draft"}),
        ),
    )
    .await
    .unwrap();

    let results = TestArticle::filter(&pool)
        .json_has_key("metadata", "views")
        .all()
        .await
        .expect("json_has_key failed");

    assert_eq!(results.len(), 1);
    assert!(results[0].title.as_str().contains("With views"));

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_pg_json_field_eq() {
    let pool = pool().await;
    setup(&pool).await;

    insert(
        &pool,
        article("Active", "content", serde_json::json!({"status": "active"})),
    )
    .await
    .unwrap();
    insert(
        &pool,
        article(
            "Inactive",
            "content",
            serde_json::json!({"status": "inactive"}),
        ),
    )
    .await
    .unwrap();

    let results = TestArticle::filter(&pool)
        .json_field_eq("metadata", "status", "active")
        .all()
        .await
        .expect("json_field_eq failed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title.as_str(), "Active");

    teardown(&pool).await;
}

// ── Export / Import CLI ───────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_export_import_roundtrip() {
    let pool = pool().await;
    setup(&pool).await;

    // Insert test data
    insert(&pool, user("roundtrip1@test.com")).await.unwrap();
    insert(&pool, user("roundtrip2@test.com")).await.unwrap();

    // Export via values() — mirrors rango export behavior
    let rows = TestUser::filter(&pool)
        .values(&["id", "email", "name", "active"])
        .await
        .expect("export via values failed");

    assert_eq!(rows.len(), 2);

    // Verify exported data is complete
    let emails: Vec<String> = rows
        .iter()
        .filter_map(|r| match r.get("email") {
            Some(SqlValue::Text(s)) => Some(s.clone()),
            _ => None,
        })
        .collect();

    assert!(emails.contains(&"roundtrip1@test.com".to_string()));
    assert!(emails.contains(&"roundtrip2@test.com".to_string()));

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_values_all_types() {
    let pool = pool().await;
    setup(&pool).await;

    let mut u = user("types@test.com");
    u.score = Some(FieldInt(42));
    insert(&pool, u).await.unwrap();

    let rows = TestUser::filter(&pool)
        .eq("email", "types@test.com")
        .values(&["email", "score", "active"])
        .await
        .expect("values failed");

    assert_eq!(rows.len(), 1);

    // Check integer type
    assert_eq!(rows[0].get("score"), Some(&SqlValue::BigInt(42)));

    // Check bool type
    assert_eq!(rows[0].get("active"), Some(&SqlValue::Bool(true)));

    teardown(&pool).await;
}

// ── Typed errors ──────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_typed_errors() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("dup@test.com")).await.unwrap();
    let err = insert(&pool, user("dup@test.com")).await.unwrap_err();
    assert!(
        matches!(
            err,
            Error::UniqueViolation {
                constraint: Some(_)
            }
        ),
        "{err:?}"
    );
    assert!(
        !err.to_string().contains("INSERT"),
        "error leaks SQL: {err}"
    );

    let err = atomic(&pool, |tx| {
        Box::pin(async move {
            insert(&mut **tx, user("aborted@test.com")).await?;
            Err::<(), _>(Error::abort("business rule"))
        })
    })
    .await
    .unwrap_err();
    assert!(matches!(err, Error::Aborted(_)));
    let exists = TestUser::filter(&pool)
        .eq("email", "aborted@test.com")
        .exists()
        .await
        .unwrap();
    assert!(!exists, "atomic must roll back on Err");

    teardown(&pool).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_atomic_retry() {
    let pool = pool().await;
    setup(&pool).await;

    let counter = std::sync::atomic::AtomicU32::new(0);

    let result = atomic_retry(&pool, 5, |tx| {
        let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        Box::pin(async move {
            insert(&mut **tx, user("retry@test.com")).await?;
            if n < 3 { Err(Error::Conflict) } else { Ok(()) }
        })
    })
    .await;
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 3);
    let count = TestUser::filter(&pool)
        .eq("email", "retry@test.com")
        .count()
        .await
        .unwrap();
    assert_eq!(count, 1);

    let counter = std::sync::atomic::AtomicU32::new(0);
    let err = atomic_retry(&pool, 2, |tx| {
        let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let _ = tx;
        Box::pin(async move {
            let _ = n;
            Err::<(), _>(Error::Conflict)
        })
    })
    .await
    .unwrap_err();
    assert!(matches!(err, Error::Conflict), "{err:?}");
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);

    let counter = std::sync::atomic::AtomicU32::new(0);
    let err = atomic_retry(&pool, 5, |tx| {
        let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let _ = tx;
        Box::pin(async move {
            let _ = n;
            Err::<(), _>(Error::abort("no"))
        })
    })
    .await
    .unwrap_err();
    assert!(matches!(err, Error::Aborted(_)), "{err:?}");
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);

    teardown(&pool).await;
}

// ── Connection settings ───────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_connect_applies_timeouts() {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/rango_test".to_string());
    let mut config = DatabaseConfig::from_url(url);
    config.statement_timeout_ms = Some(1234);
    config.lock_timeout_ms = Some(567);
    let pool = connect(&config).await.unwrap();

    let st: String = sqlx::query_scalar("SHOW statement_timeout")
        .fetch_one(&pool)
        .await
        .unwrap();
    let lt: String = sqlx::query_scalar("SHOW lock_timeout")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((st.as_str(), lt.as_str()), ("1234ms", "567ms"));

    let err = sqlx::query("SELECT pg_sleep(2)")
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())
        .unwrap_err();
    assert!(err.contains("statement timeout"), "{err}");
}

// ── Auto-increment PK ─────────────────────────────────────────────────────────

#[derive(Model, Debug, Clone)]
#[model(table = "rango_test_counter")]
struct TestCounter {
    id: FieldAutoBigInt,
    label: FieldText,
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_auto_pk_is_generated() {
    let pool = pool().await;
    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_counter""#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        r#"CREATE TABLE "rango_test_counter" (
            "id" BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
            "label" TEXT NOT NULL
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    let new = |l: &str| TestCounter {
        id: FieldAutoBigInt::default(),
        label: FieldText(l.into()),
    };

    let a = insert(&pool, new("a")).await.unwrap();
    let b = insert(&pool, new("b")).await.unwrap();
    assert_eq!((a.id.0, b.id.0), (Some(1), Some(2)));

    assert_eq!(bulk_create(&pool, &[new("c"), new("d")]).await.unwrap(), 2);
    let d = get::<_, TestCounter>(&pool, &4i64.into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(d.label.0, "d");
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
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
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_pg_select_as_partial() {
    let pool = pool().await;
    sqlx::query(r#"DROP TABLE IF EXISTS "rango_test_partial_person""#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(r#"CREATE TABLE "rango_test_partial_person" ("id" UUID PRIMARY KEY, "full_name" VARCHAR(100) NOT NULL, "age" INTEGER NOT NULL, "nick" TEXT)"#).execute(&pool).await.unwrap();

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

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
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

    teardown(&pool).await;
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
#[model(table = "rango_test_hooked", hooks)]
struct Hooked {
    id: FieldUuid,
    name: FieldVarchar<1, 100>,
    slug: FieldVarchar<0, 100>,
}

impl ModelHooks for Hooked {
    fn before_save(&mut self) -> BoxFuture<'_> {
        Box::pin(async move {
            self.slug = FieldVarchar(self.name.0.to_lowercase());
            hook_log("before_save", &self.name.0);
            Ok(())
        })
    }
    fn after_save(&self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("after_save", &self.name.0);
            if self.name.0.starts_with("fail_after") {
                return Err("after_save refused".into());
            }
            Ok(())
        })
    }
    fn before_create(&mut self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("before_create", &self.name.0);
            if self.name.0.starts_with("fail_create") {
                return Err("before_create refused".into());
            }
            Ok(())
        })
    }
    fn after_create(&self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("after_create", &self.name.0);
            Ok(())
        })
    }
    fn before_update(&mut self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("before_update", &self.name.0);
            Ok(())
        })
    }
    fn after_update(&self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("after_update", &self.name.0);
            Ok(())
        })
    }
    fn before_delete(&self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("before_delete", &self.name.0);
            Ok(())
        })
    }
    fn after_delete(&self) -> BoxFuture<'_> {
        Box::pin(async move {
            hook_log("after_delete", &self.name.0);
            Ok(())
        })
    }
}

fn hooked(name: &str) -> Hooked {
    Hooked {
        id: FieldUuid(Uuid::new_v4()),
        name: FieldVarchar(name.to_string()),
        slug: FieldVarchar(String::new()),
    }
}

async fn hooked_setup(pool: &PgPool) {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS "rango_test_hooked" (
            "id"   UUID PRIMARY KEY,
            "name" VARCHAR(100) NOT NULL UNIQUE,
            "slug" VARCHAR(100) NOT NULL
        )"#,
    )
    .execute(pool)
    .await
    .expect("setup: create hooked table");
}

async fn hooked_count(pool: &PgPool, name: &str) -> usize {
    Hooked::filter(pool)
        .eq("name", name)
        .all()
        .await
        .unwrap()
        .len()
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_hooks_insert_update_delete() {
    let pool = pool().await;
    hooked_setup(&pool).await;

    let saved = insert(&pool, hooked("Ada Lovelace")).await.unwrap();
    // before_save mutated the model and the mutation is what was written
    assert_eq!(saved.slug.0, "ada lovelace");
    let stored = get::<_, Hooked>(&pool, &saved.id.to_sql_value())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.slug.0, "ada lovelace");
    assert_eq!(
        hook_events("Ada Lovelace"),
        ["before_save", "before_create", "after_create", "after_save"]
    );

    // update: save + update hooks, never create hooks
    let mut changed = saved.clone();
    changed.slug = FieldVarchar("tampered".into());
    let updated = update(&pool, changed).await.unwrap();
    assert_eq!(updated.slug.0, "ada lovelace");
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
#[cfg_attr(not(feature = "integration"), ignore)]
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
#[cfg_attr(not(feature = "integration"), ignore)]
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
#[cfg_attr(not(feature = "integration"), ignore)]
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
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_model_without_hooks_attribute_still_works() {
    let pool = pool().await;
    setup(&pool).await;
    let saved = insert(&pool, user("nohooks@hooks.com")).await.unwrap();
    delete(&pool, &saved).await.unwrap();
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn test_bulk_create_is_atomic_across_chunks() {
    let pool = pool().await;
    setup(&pool).await;

    // Enough rows for several chunks; the last row duplicates the first email.
    let mut users: Vec<TestUser> = (0..100)
        .map(|i| user(&format!("atomic{}@t.com", i)))
        .collect();
    users.push(user("atomic0@t.com"));

    let err = bulk_create(&pool, &users).await;
    assert!(err.is_err());
    assert_eq!(TestUser::filter(&pool).count().await.unwrap(), 0);
}
