/// MySQL/MariaDB integration tests — run against a local Docker database.
///
/// Run with:
///   docker compose --profile mysql up -d --wait
///   MYSQL_URL=mysql://root:rango@127.0.0.1:3307/rango_test cargo test --test integration_mysql --features rango-tests/integration-mysql -- --test-threads=1
///
/// The same file runs against MySQL 8 (port 3307) and MariaDB (port 3308).
use rango_orm_core::*;
use rango_orm_derive::Model;
use rango_orm_mysql::*;
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

async fn pool() -> MySqlPool {
    let url = std::env::var("MYSQL_URL")
        .unwrap_or_else(|_| "mysql://root:rango@127.0.0.1:3307/rango_test".to_string());
    rango_orm_mysql::connect(&url)
        .await
        .expect("Failed to create MySQL pool")
}

async fn setup(pool: &MySqlPool) {
    // test_post references test_user: drop it first.
    sqlx::query("DROP TABLE IF EXISTS `test_post`")
        .execute(pool)
        .await
        .expect("setup failed");
    sqlx::query("DROP TABLE IF EXISTS `test_user`")
        .execute(pool)
        .await
        .expect("setup failed");

    sqlx::query(
        r#"CREATE TABLE `test_user` (
            `id`     BINARY(16) PRIMARY KEY,
            `email`  VARCHAR(255) NOT NULL UNIQUE,
            `name`   VARCHAR(255) NOT NULL,
            `active` BOOLEAN NOT NULL,
            `score`  INT
        ) ENGINE=InnoDB"#,
    )
    .execute(pool)
    .await
    .expect("setup failed");
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_prefetch_related_many_to_many() {
    let pool = pool().await;

    sqlx::query(r#"DROP TABLE IF EXISTS `test_blog_test_tag`"#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(r#"DROP TABLE IF EXISTS `test_blog`"#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(r#"DROP TABLE IF EXISTS `test_tag`"#)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query(
        r#"CREATE TABLE `test_blog` (
            `id`    BINARY(16) PRIMARY KEY,
            `title` VARCHAR(100) NOT NULL
        ) ENGINE=InnoDB"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"CREATE TABLE `test_tag` (
            `id`   BINARY(16) PRIMARY KEY,
            `name` VARCHAR(100) NOT NULL
        ) ENGINE=InnoDB"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"CREATE TABLE `test_blog_test_tag` (
            `test_blog_id` BINARY(16) NOT NULL,
            `test_tag_id`  BINARY(16) NOT NULL,
            PRIMARY KEY (`test_blog_id`, `test_tag_id`)
        ) ENGINE=InnoDB"#,
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

    sqlx::query(r#"DROP TABLE IF EXISTS `test_blog_test_tag`"#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(r#"DROP TABLE IF EXISTS `test_blog`"#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(r#"DROP TABLE IF EXISTS `test_tag`"#)
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

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_select_and_prefetch_related() {
    let pool = pool().await;
    setup(&pool).await;
    sqlx::query(
        r#"CREATE TABLE `test_post` (
            `id`        BINARY(16) PRIMARY KEY,
            `author`    BINARY(16) NOT NULL,
            `title`     VARCHAR(100) NOT NULL,
            FOREIGN KEY (`author`) REFERENCES `test_user` (`id`)
        ) ENGINE=InnoDB"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let alice = insert(&pool, user("alice@mysql.com")).await.unwrap();
    let bob = insert(&pool, user("bob@mysql.com")).await.unwrap();
    for title in ["first", "second"] {
        let post = TestPost {
            id: FieldUuid(Uuid::new_v4()),
            author: ForeignKey::new(alice.id.0),
            title: FieldVarchar::new(title.to_string()).unwrap(),
        };
        insert(&pool, post).await.unwrap();
    }

    let mut q = TestPost::filter(&pool)
        .select_related(TestPost::AUTHOR)
        .order_by("-title")
        .limit(1);
    assert!(q.explain().contains("LEFT JOIN"), "{}", q.explain());
    // MySQL: EXPLAIN ANALYZE tree; MariaDB: ANALYZE FORMAT=JSON (r_* fields).
    let plan = q.explain_analyze().await.unwrap();
    assert!(
        plan.contains("actual time") || plan.contains("\"r_loops\""),
        "{plan}"
    );
    let posts = q.all().await.unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].title.as_str(), "second");
    assert_eq!(posts[0].related::<TestUser>().email, alice.email);

    let users = TestUser::filter(&pool)
        .prefetch_related(TestPost::AUTHOR)
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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_insert_and_get() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("insert@mysql.com");
    let inserted = insert(&pool, u.clone()).await.expect("insert failed");
    assert_eq!(inserted.email, u.email);

    let fetched = get::<_, TestUser>(&pool, &u.id.to_sql_value())
        .await
        .expect("get failed")
        .expect("user not found");
    assert_eq!(fetched.email, u.email);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_update() {
    let pool = pool().await;
    setup(&pool).await;

    let mut u = user("update@mysql.com");
    insert(&pool, u.clone()).await.unwrap();

    u.name = FieldVarchar::new("Updated".to_string()).unwrap();
    let updated = update(&pool, u.clone()).await.expect("update failed");
    assert_eq!(
        updated.name,
        FieldVarchar::new("Updated".to_string()).unwrap()
    );
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_delete() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("delete@mysql.com");
    insert(&pool, u.clone()).await.unwrap();
    delete(&pool, &u).await.expect("delete failed");

    let fetched = get::<_, TestUser>(&pool, &u.id.to_sql_value())
        .await
        .unwrap();
    assert!(fetched.is_none());
}

// ── Query builder ─────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_filter_eq() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("active@mysql.com")).await.unwrap();
    let mut inactive = user("inactive@mysql.com");
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
            .any(|u| u.email == FieldEmail::new("active@mysql.com".to_string()).unwrap())
    );
    assert!(
        !results
            .iter()
            .any(|u| u.email == FieldEmail::new("inactive@mysql.com".to_string()).unwrap())
    );
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_subqueries() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("a@mysql.com")).await.unwrap();
    let mut inactive = user("b@mysql.com");
    inactive.active = FieldBool(false);
    insert(&pool, inactive).await.unwrap();

    // Self-referencing EXISTS: exercises the inner-table alias and bind order.
    let active = TestUser::filter(&pool)
        .eq("email", "b@mysql.com".to_string())
        .or()
        .where_exists::<TestUser>("id", |q| q.eq("active", true))
        .order_by("email")
        .all()
        .await
        .unwrap();
    assert_eq!(
        active.iter().map(|u| u.email.as_str()).collect::<Vec<_>>(),
        ["a@mysql.com", "b@mysql.com"]
    );

    let inactive = TestUser::filter(&pool)
        .not()
        .in_subquery(TestUser::ID, TestUser::ID, |q| q.eq("active", true))
        .all()
        .await
        .unwrap();
    assert_eq!(
        inactive
            .iter()
            .map(|u| u.email.as_str())
            .collect::<Vec<_>>(),
        ["b@mysql.com"]
    );
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_count_and_exists() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("count1@mysql.com")).await.unwrap();
    insert(&pool, user("count2@mysql.com")).await.unwrap();

    let count = TestUser::filter(&pool).count().await.unwrap();
    assert!(count >= 2);

    let exists = TestUser::filter(&pool)
        .eq("email", "count1@mysql.com")
        .exists()
        .await
        .unwrap();
    assert!(exists);
}

// ── Bulk ops ──────────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_bulk_create() {
    let pool = pool().await;
    setup(&pool).await;

    let users: Vec<TestUser> = (0..5)
        .map(|i| user(&format!("bulk{}@mysql.com", i)))
        .collect();

    let n = bulk_create(&pool, &users)
        .await
        .expect("bulk_create failed");
    assert_eq!(n, 5);
}

// ── Transactions ──────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_atomic_commit() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("atomic@mysql.com");
    let mut tx = pool.begin().await.unwrap();
    insert(&mut *tx, u).await.expect("insert in tx failed");
    tx.commit().await.unwrap();

    let exists = TestUser::filter(&pool)
        .eq("email", "atomic@mysql.com")
        .exists()
        .await
        .unwrap();
    assert!(exists);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_atomic_rollback() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("rollback@mysql.com");
    let mut tx = pool.begin().await.unwrap();
    insert(&mut *tx, u).await.expect("insert in tx failed");
    tx.rollback().await.unwrap();

    let exists = TestUser::filter(&pool)
        .eq("email", "rollback@mysql.com")
        .exists()
        .await
        .unwrap();
    assert!(!exists);
}

// ── Aggregations ──────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_aggregations() {
    let pool = pool().await;
    setup(&pool).await;

    let mut u1 = user("agg1@mysql.com");
    u1.score = Some(FieldInt(10));
    let mut u2 = user("agg2@mysql.com");
    u2.score = Some(FieldInt(20));
    let mut u3 = user("agg3@mysql.com");
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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_raw_scalar() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("raw@mysql.com")).await.unwrap();

    let count = raw_scalar::<_, i64>(
        &pool,
        "SELECT COUNT(*) FROM `test_user` WHERE email = ?",
        vec![SqlValue::Text("raw@mysql.com".to_string())],
    )
    .await
    .expect("raw_scalar failed");

    assert_eq!(count, 1);
}

/// No `rango::call`: a procedure is plain SQL through `raw*`, values bound.
#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_stored_procedure_via_raw() {
    let pool = pool().await;
    // MySQL 8.4 cannot prepare procedure DDL: run it over the text protocol.
    sqlx::raw_sql(
        "DROP PROCEDURE IF EXISTS rango_test_double;
         CREATE PROCEDURE rango_test_double(IN x INT) SELECT x * 2 AS doubled",
    )
    .execute(&pool)
    .await
    .unwrap();

    // The procedure's result set comes back like a SELECT.
    let doubled: i64 = raw_scalar(&pool, "CALL rango_test_double(?)", vec![SqlValue::Int(21)])
        .await
        .unwrap();
    assert_eq!(doubled, 42);

    sqlx::raw_sql("DROP PROCEDURE rango_test_double")
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_values_projection() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("values@mysql.com")).await.unwrap();

    let rows = TestUser::filter(&pool)
        .eq("email", "values@mysql.com")
        .values(&["email", "active"])
        .await
        .expect("values failed");

    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains_key("email"));
    assert!(rows[0].contains_key("active"));
    assert!(!rows[0].contains_key("name"));

    if let SqlValue::Text(email) = &rows[0]["email"] {
        assert_eq!(email, "values@mysql.com");
    } else {
        panic!("email should be SqlValue::Text");
    }
}

// ── Export / Import ───────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_export_json() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("export1@mysql.com")).await.unwrap();
    insert(&pool, user("export2@mysql.com")).await.unwrap();

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

    assert!(json.contains("export1@mysql.com"));
    assert!(json.contains("export2@mysql.com"));
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_roundtrip() {
    // Insert → export via values() → verify data integrity
    let pool = pool().await;
    setup(&pool).await;

    let u = user("roundtrip@mysql.com");
    insert(&pool, u.clone()).await.unwrap();

    let rows = TestUser::filter(&pool)
        .eq("email", "roundtrip@mysql.com")
        .values(&["email", "name"])
        .await
        .expect("values failed");

    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("email"),
        Some(&rango_orm_core::SqlValue::Text(
            "roundtrip@mysql.com".to_string()
        ))
    );
    assert_eq!(
        rows[0].get("name"),
        Some(&rango_orm_core::SqlValue::Text("Test User".to_string()))
    );
}

// ── MySQL-specific ────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_update_not_found() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("notfound@mysql.com");
    let err = update(&pool, u).await.unwrap_err();
    assert!(matches!(err, rango_orm_core::Error::NotFound), "{err:?}");
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_update_unchanged() {
    let pool = pool().await;
    setup(&pool).await;

    let u = user("unchanged@mysql.com");
    insert(&pool, u.clone()).await.unwrap();

    let updated = update(&pool, u.clone())
        .await
        .expect("update with unchanged values failed");
    assert_eq!(updated.email, u.email);
    assert_eq!(updated.name, u.name);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_unique_violation_is_typed() {
    let pool = pool().await;
    setup(&pool).await;

    insert(&pool, user("dup@mysql.com")).await.unwrap();
    let err = insert(&pool, user("dup@mysql.com")).await.unwrap_err();
    assert!(
        matches!(err, rango_orm_core::Error::UniqueViolation { .. }),
        "{err:?}"
    );
}

// ── Typed columns ─────────────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_typed_columns() {
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

// ── Type coverage, LIKE semantics, get_or_create, FK cascade ────────────────────

#[derive(Model, Debug, Clone, PartialEq)]
#[model(table = "test_types")]
struct TestTypes {
    id: FieldUuid,
    flag: FieldBool,
    small: FieldSmallInt,
    big: FieldBigInt,
    ratio: FieldFloat,
    precise: FieldDouble,
    body: FieldText,
    blob: FieldBytes,
    day: FieldDate,
    at_time: FieldTime,
    at: FieldDateTime,
    meta: FieldJson,
    other: Option<FieldUuid>,
    maybe_at: Option<FieldDateTime>,
}

#[derive(Model, Debug, Clone, PartialEq)]
#[model(table = "test_post")]
struct TestPost {
    id: FieldUuid,
    author: ForeignKey<TestUser>,
    title: FieldVarchar<1, 100>,
}

async fn exec(pool: &MySqlPool, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(pool)
        .await
        .expect(sql);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_all_types_roundtrip() {
    use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
    let pool = pool().await;
    exec(&pool, "DROP TABLE IF EXISTS `test_types`").await;
    exec(
        &pool,
        "CREATE TABLE `test_types` (
            `id` BINARY(16) PRIMARY KEY, `flag` BOOLEAN NOT NULL, `small` SMALLINT NOT NULL,
            `big` BIGINT NOT NULL, `ratio` FLOAT NOT NULL, `precise` DOUBLE NOT NULL,
`body` TEXT NOT NULL, `blob` LONGBLOB NOT NULL,
            `day` DATE NOT NULL, `at_time` TIME(6) NOT NULL, `at` DATETIME(6) NOT NULL,
            `meta` JSON NOT NULL, `other` BINARY(16), `maybe_at` DATETIME(6)
        ) ENGINE=InnoDB",
    )
    .await;

    let row = TestTypes {
        id: FieldUuid(Uuid::new_v4()),
        flag: FieldBool(true),
        small: FieldSmallInt(-7),
        big: FieldBigInt(i64::MAX),
        ratio: FieldFloat(0.5),
        precise: FieldDouble(1.0 / 3.0),
        body: FieldText("héllo 🦎".into()),
        blob: FieldBytes(vec![0, 1, 254, 255]),
        day: FieldDate(NaiveDate::from_ymd_opt(2026, 11, 5).unwrap()),
        at_time: FieldTime(NaiveTime::from_hms_micro_opt(13, 37, 0, 123_456).unwrap()),
        at: FieldDateTime(
            Utc.with_ymd_and_hms(2026, 11, 5, 22, 0, 0).unwrap()
                + chrono::Duration::microseconds(654_321),
        ),
        meta: FieldJson(serde_json::json!({"k": [1, "two", null]})),
        other: None,
        maybe_at: None,
    };
    insert(&pool, row.clone()).await.unwrap();
    let back: TestTypes = get(&pool, &row.pk_value()).await.unwrap().unwrap();
    assert_eq!(back, row);

    let found = TestTypes::filter(&pool)
        .eq(TestTypes::ID, row.id.0)
        .one()
        .await
        .unwrap();
    assert!(found.is_some(), "UUID bound as BINARY(16) in filters");
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_like_is_case_sensitive_ilike_is_not() {
    let pool = pool().await;
    setup(&pool).await;
    let mut u = user("Alice@Example.com");
    u.name = FieldVarchar::new("Alice").unwrap();
    insert(&pool, u).await.unwrap();

    let n = TestUser::filter(&pool)
        .like("name", "ali%")
        .count()
        .await
        .unwrap();
    assert_eq!(n, 0);
    let n = TestUser::filter(&pool)
        .like("name", "Ali%")
        .count()
        .await
        .unwrap();
    assert_eq!(n, 1);
    let n = TestUser::filter(&pool)
        .ilike("name", "ALI%")
        .count()
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_get_or_create_both_paths() {
    let pool = pool().await;
    setup(&pool).await;
    let lookup = || vec![("email", SqlValue::Text("goc@mysql.com".into()))];

    let (first, created) = get_or_create(&pool, lookup(), user("goc@mysql.com"))
        .await
        .unwrap();
    assert!(created);
    let (second, created) = get_or_create(&pool, lookup(), user("goc@mysql.com"))
        .await
        .unwrap();
    assert!(!created);
    assert_eq!(first.id, second.id);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_fk_cascade_and_violation() {
    let pool = pool().await;
    exec(&pool, "DROP TABLE IF EXISTS `test_post`").await;
    setup(&pool).await;
    exec(
        &pool,
        "CREATE TABLE `test_post` (
            `id` BINARY(16) PRIMARY KEY, `author` BINARY(16) NOT NULL, `title` VARCHAR(100) NOT NULL,
            FOREIGN KEY (`author`) REFERENCES `test_user` (`id`) ON DELETE CASCADE
        ) ENGINE=InnoDB",
    )
    .await;

    let u = insert(&pool, user("fk@mysql.com")).await.unwrap();
    let post = TestPost {
        id: FieldUuid(Uuid::new_v4()),
        author: ForeignKey::new(u.id.0),
        title: FieldVarchar::new("hello").unwrap(),
    };
    insert(&pool, post).await.unwrap();

    let orphan = TestPost {
        id: FieldUuid(Uuid::new_v4()),
        author: ForeignKey::new(Uuid::new_v4()),
        title: FieldVarchar::new("orphan").unwrap(),
    };
    let err = insert(&pool, orphan).await.unwrap_err();
    assert!(
        matches!(err, rango_orm_core::Error::ForeignKeyViolation { .. }),
        "{err:?}"
    );

    delete(&pool, &u).await.unwrap();
    let left: i64 = raw_scalar(&pool, "SELECT COUNT(*) FROM `test_post`", vec![])
        .await
        .unwrap();
    assert_eq!(left, 0, "ON DELETE CASCADE");
    exec(&pool, "DROP TABLE `test_post`").await;
}

// ── Auto-increment PK ─────────────────────────────────────────────────────────

#[derive(Model, Debug, Clone)]
#[model(table = "test_counter")]
struct TestCounter {
    id: FieldAutoBigInt,
    #[field(unique)]
    label: FieldVarchar<1, 50>,
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_auto_pk_is_generated() {
    let pool = pool().await;
    sqlx::query("DROP TABLE IF EXISTS `test_counter`")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(
        "CREATE TABLE `test_counter` (`id` BIGINT AUTO_INCREMENT PRIMARY KEY, \
         `label` VARCHAR(50) NOT NULL UNIQUE) ENGINE=InnoDB",
    ))
    .execute(&pool)
    .await
    .unwrap();
    let new = |l: &str| TestCounter {
        id: FieldAutoBigInt::default(),
        label: FieldVarchar::new(l).unwrap(),
    };

    let a = insert(&pool, new("a")).await.unwrap();
    let b = insert(&pool, new("b")).await.unwrap();
    assert_eq!((a.id.0, b.id.0), (Some(1), Some(2)));

    let (c, created) = get_or_create(&pool, vec![("label", "c".into())], new("c"))
        .await
        .unwrap();
    assert!(created);
    assert_eq!(c.id.0, Some(3));

    assert_eq!(bulk_create(&pool, &[new("d"), new("e")]).await.unwrap(), 2);
    let e = get::<_, TestCounter>(&pool, &5i64.into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(e.label.as_str(), "e");
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_select_as_partial() {
    let pool = pool().await;
    sqlx::query(r#"DROP TABLE IF EXISTS `rango_test_partial_person`"#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(r#"CREATE TABLE `rango_test_partial_person` (`id` BINARY(16) PRIMARY KEY, `full_name` VARCHAR(100) NOT NULL, `age` INT NOT NULL, `nick` TEXT) ENGINE=InnoDB"#).execute(&pool).await.unwrap();

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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
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

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_bulk_create_is_atomic_across_chunks() {
    let pool = pool().await;
    setup(&pool).await;

    // Enough rows for several chunks; the last row duplicates the first email.
    let mut users: Vec<TestUser> = (0..14000)
        .map(|i| user(&format!("atomic{}@t.com", i)))
        .collect();
    users.push(user("atomic0@t.com"));

    let err = bulk_create(&pool, &users).await;
    assert!(err.is_err());
    assert_eq!(TestUser::filter(&pool).count().await.unwrap(), 0);
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_in_values_empty_list() {
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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
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

async fn hooked_setup(pool: &MySqlPool) {
    sqlx::query("DROP TABLE IF EXISTS `test_hooked`")
        .execute(pool)
        .await
        .expect("setup: drop hooked table");
    sqlx::query(
        "CREATE TABLE `test_hooked` (
            `id`   BINARY(16) PRIMARY KEY,
            `name` VARCHAR(100) NOT NULL UNIQUE,
            `slug` VARCHAR(100) NOT NULL
        ) ENGINE=InnoDB",
    )
    .execute(pool)
    .await
    .expect("setup: create hooked table");
}

async fn hooked_count(pool: &MySqlPool, name: &str) -> usize {
    Hooked::filter(pool)
        .eq("name", name)
        .all()
        .await
        .unwrap()
        .len()
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_model_without_hooks_attribute_still_works() {
    let pool = pool().await;
    setup(&pool).await;
    let saved = insert(&pool, user("nohooks@hooks.com")).await.unwrap();
    delete(&pool, &saved).await.unwrap();
}

// ── Transactions everywhere: filter + M2M + bulk in one rolled-back atomic ───────

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_filter_m2m_bulk_roll_back_in_atomic() {
    let pool = pool().await;
    setup(&pool).await;

    for t in ["test_blog_test_tag", "test_blog", "test_tag"] {
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS `{t}`")))
            .execute(&pool)
            .await
            .unwrap();
    }
    for ddl in [
        "CREATE TABLE `test_blog` (`id` BINARY(16) PRIMARY KEY, `title` VARCHAR(100) NOT NULL) ENGINE=InnoDB",
        "CREATE TABLE `test_tag` (`id` BINARY(16) PRIMARY KEY, `name` VARCHAR(100) NOT NULL) ENGINE=InnoDB",
        "CREATE TABLE `test_blog_test_tag` (`test_blog_id` BINARY(16) NOT NULL, `test_tag_id` BINARY(16) NOT NULL, PRIMARY KEY (`test_blog_id`, `test_tag_id`)) ENGINE=InnoDB",
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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_bulk_update_bulk_upsert_update_or_create() {
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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn test_mysql_atomic_future_is_send() {
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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn mysql_test_fk_and_m2m_follow_the_target_pk_type() {
    let pool = pool().await;
    for ddl in [
        "DROP TABLE IF EXISTS `pk_book_pk_label`",
        "DROP TABLE IF EXISTS `pk_book`",
        "DROP TABLE IF EXISTS `pk_label`",
        "DROP TABLE IF EXISTS `pk_author`",
        "CREATE TABLE `pk_author` (`id` BIGINT AUTO_INCREMENT PRIMARY KEY, `name` VARCHAR(100) NOT NULL)",
        "CREATE TABLE `pk_label` (`id` INT AUTO_INCREMENT PRIMARY KEY, `name` VARCHAR(100) NOT NULL)",
        "CREATE TABLE `pk_book` (`id` BINARY(16) PRIMARY KEY, `author_id` BIGINT NOT NULL, `editor_id` INT NULL, `title` VARCHAR(100) NOT NULL, FOREIGN KEY (`author_id`) REFERENCES `pk_author` (`id`), FOREIGN KEY (`editor_id`) REFERENCES `pk_label` (`id`))",
        "CREATE TABLE `pk_book_pk_label` (`pk_book_id` BINARY(16) NOT NULL, `pk_label_id` INT NOT NULL, PRIMARY KEY (`pk_book_id`, `pk_label_id`), FOREIGN KEY (`pk_book_id`) REFERENCES `pk_book` (`id`) ON DELETE CASCADE, FOREIGN KEY (`pk_label_id`) REFERENCES `pk_label` (`id`) ON DELETE CASCADE)",
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
