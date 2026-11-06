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
        title: FieldVarchar("rust".to_string()),
        tags: ManyToMany::empty(),
    };
    let b2 = TestBlog {
        id: FieldUuid(Uuid::new_v4()),
        title: FieldVarchar("empty".to_string()),
        tags: ManyToMany::empty(),
    };
    let t1 = TestTag {
        id: FieldUuid(Uuid::new_v4()),
        name: FieldVarchar("orm".to_string()),
    };
    let t2 = TestTag {
        id: FieldUuid(Uuid::new_v4()),
        name: FieldVarchar("async".to_string()),
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
        .map(|t| t.name.0.clone())
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
        email: FieldEmail(email.to_string()),
        name: FieldVarchar("Test User".to_string()),
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
            title: FieldVarchar(title.to_string()),
        };
        insert(&pool, post).await.unwrap();
    }

    let q = TestPost::filter(&pool)
        .select_related(TestPost::AUTHOR)
        .order_by("-title")
        .limit(1);
    assert!(q.explain().contains("LEFT JOIN"), "{}", q.explain());
    let posts = q.all().await.unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].title.0, "second");
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

    u.name = FieldVarchar("Updated".to_string());
    let updated = update(&pool, u.clone()).await.expect("update failed");
    assert_eq!(updated.name, FieldVarchar("Updated".to_string()));
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
            .any(|u| u.email == FieldEmail("active@mysql.com".to_string()))
    );
    assert!(
        !results
            .iter()
            .any(|u| u.email == FieldEmail("inactive@mysql.com".to_string()))
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
        active
            .iter()
            .map(|u| u.email.0.as_str())
            .collect::<Vec<_>>(),
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
            .map(|u| u.email.0.as_str())
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
    assert_eq!(desc[0].email, FieldEmail("b@typed.com".to_string()));

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
    sqlx::query(sql).execute(pool).await.expect(sql);
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
    u.name = FieldVarchar("Alice".into());
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
        title: FieldVarchar("hello".into()),
    };
    insert(&pool, post).await.unwrap();

    let orphan = TestPost {
        id: FieldUuid(Uuid::new_v4()),
        author: ForeignKey::new(Uuid::new_v4()),
        title: FieldVarchar("orphan".into()),
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
    sqlx::query(
        "CREATE TABLE `test_counter` (`id` BIGINT AUTO_INCREMENT PRIMARY KEY, \
         `label` VARCHAR(50) NOT NULL UNIQUE) ENGINE=InnoDB",
    )
    .execute(&pool)
    .await
    .unwrap();
    let new = |l: &str| TestCounter {
        id: FieldAutoBigInt::default(),
        label: FieldVarchar(l.into()),
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
    assert_eq!(e.label.0, "e");
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
