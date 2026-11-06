//! End-to-end migration tests for the `rango` CLI against PostgreSQL.
//!
//! Run with: `cargo test --test migrations_e2e --features integration -- --test-threads=1`

use std::path::PathBuf;

use sqlx::PgPool;

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5433/rango_test".into())
}

struct Project {
    dir: PathBuf,
    prefix: String,
}

impl Project {
    fn new(name: &str) -> Self {
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("rango_e2e_{name}_{pid}"));
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        std::fs::create_dir_all(dir.join("src")).expect("create project src dir");

        Self {
            dir,
            prefix: format!("e2e_{name}"),
        }
    }

    fn write_models(&self, body: &str) {
        // Isolate each test with its own table (no package prefix any more).
        let body = body.replace(
            "pub struct Item {",
            &format!("#[model(table = \"{}\")]\npub struct Item {{", self.table()),
        );
        let content = format!("use rango::prelude::*;\n\n{body}");
        std::fs::write(self.dir.join("src/models.rs"), content).expect("write models.rs");
    }

    fn makemigrations(&self) -> std::process::Output {
        self.run_cli(&["makemigrations", "src", "-o", "migrations", "--yes"])
    }

    fn migrate(&self) -> std::process::Output {
        self.run_cli(&["migrate"])
    }

    fn run_cli(&self, args: &[&str]) -> std::process::Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_rango"))
            .current_dir(&self.dir)
            .env("DATABASE_URL", database_url())
            .stdin(std::process::Stdio::null())
            .args(args)
            .output()
            .expect("run rango CLI")
    }

    fn migration_sql_files(&self) -> Vec<PathBuf> {
        let migrations_dir = self.dir.join("migrations");
        let mut files = std::fs::read_dir(&migrations_dir)
            .expect("read migrations dir")
            .map(|entry| entry.expect("read migration entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
            .collect::<Vec<_>>();
        files.sort();
        files
    }

    fn latest_migration_sql(&self) -> String {
        let files = self.migration_sql_files();
        let last = files.last().expect("expected at least one migration file");
        std::fs::read_to_string(last).expect("read latest migration")
    }

    fn table(&self) -> String {
        format!("{}_item", self.prefix)
    }

    fn remove(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn reset(pool: &PgPool, project: &Project) {
    let table = project.table();
    sqlx::query(&format!("DROP TABLE IF EXISTS \"{}\"", table))
        .execute(pool)
        .await
        .expect("drop project table");
    sqlx::query("DROP TABLE IF EXISTS _rango_migrations")
        .execute(pool)
        .await
        .expect("drop migration ledger");
}

async fn drop_table(pool: &PgPool, project: &Project) {
    let table = project.table();
    sqlx::query(&format!("DROP TABLE IF EXISTS \"{}\"", table))
        .execute(pool)
        .await
        .expect("drop project table");
}

async fn column_type(pool: &PgPool, table: &str, column: &str) -> Option<String> {
    let sql =
        "SELECT data_type FROM information_schema.columns WHERE table_name=$1 AND column_name=$2";
    sqlx::query_scalar(sql)
        .bind(table)
        .bind(column)
        .fetch_optional(pool)
        .await
        .expect("query column type")
}

fn assert_success(output: &std::process::Output, label: &str) {
    if !output.status.success() {
        panic!(
            "{label} failed with status {:?}: stdout={} stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn assert_failure(output: &std::process::Output, label: &str) {
    if output.status.success() {
        panic!(
            "{label} unexpectedly succeeded: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn rename_and_type_change_in_one_migration() {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to postgres");
    let project = Project::new("rename_and_type_change_in_one_migration");
    reset(&pool, &project).await;

    project.write_models(
        r#"
#[derive(Model)]
pub struct Item {
    pub id: FieldUuid,
    pub amount: FieldText,
}
"#,
    );
    assert_success(&project.makemigrations(), "initial makemigrations");
    assert_success(&project.migrate(), "initial migrate");

    let table = project.table();
    sqlx::query(&format!(
        "INSERT INTO \"{}\" (id, amount) VALUES (gen_random_uuid(), $1)",
        table
    ))
    .bind("42")
    .execute(&pool)
    .await
    .expect("insert 42");
    sqlx::query(&format!(
        "INSERT INTO \"{}\" (id, amount) VALUES (gen_random_uuid(), $1)",
        table
    ))
    .bind("7")
    .execute(&pool)
    .await
    .expect("insert 7");

    project.write_models(
        r#"
#[derive(Model)]
pub struct Item {
    pub id: FieldUuid,
    pub price: FieldInt,
}
"#,
    );
    assert_success(&project.makemigrations(), "v2 makemigrations");

    let sql = project.latest_migration_sql();
    assert!(sql.contains("RENAME COLUMN \"amount\" TO \"price\""));
    assert!(sql.contains("USING \"price\"::INTEGER"));
    assert!(!sql.contains("DROP COLUMN"));
    assert_eq!(project.migration_sql_files().len(), 2);

    assert_success(&project.migrate(), "v2 migrate");

    assert_eq!(
        column_type(&pool, &table, "price").await.as_deref(),
        Some("integer")
    );
    assert_eq!(column_type(&pool, &table, "amount").await, None);

    let prices: Vec<i32> =
        sqlx::query_scalar(&format!("SELECT price FROM \"{}\" ORDER BY price", table))
            .fetch_all(&pool)
            .await
            .expect("select prices");
    assert_eq!(prices, vec![7, 42]);

    drop_table(&pool, &project).await;
    project.remove();
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn impossible_conversion_rolls_back() {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to postgres");
    let project = Project::new("impossible_conversion_rolls_back");
    reset(&pool, &project).await;

    project.write_models(
        r#"
#[derive(Model)]
pub struct Item {
    pub id: FieldUuid,
    pub amount: FieldText,
}
"#,
    );
    assert_success(&project.makemigrations(), "initial makemigrations");
    assert_success(&project.migrate(), "initial migrate");

    let table = project.table();
    sqlx::query(&format!(
        "INSERT INTO \"{}\" (id, amount) VALUES (gen_random_uuid(), $1)",
        table
    ))
    .bind("abc")
    .execute(&pool)
    .await
    .expect("insert abc");

    project.write_models(
        r#"
#[derive(Model)]
pub struct Item {
    pub id: FieldUuid,
    pub price: FieldInt,
}
"#,
    );
    assert_success(&project.makemigrations(), "v2 makemigrations");

    assert_failure(&project.migrate(), "v2 migrate");

    assert_eq!(
        column_type(&pool, &table, "amount").await.as_deref(),
        Some("text")
    );
    assert_eq!(column_type(&pool, &table, "price").await, None);

    let rows: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM \"{}\" WHERE amount = $1",
        table
    ))
    .bind("abc")
    .fetch_one(&pool)
    .await
    .expect("count abc row");
    assert_eq!(rows, 1);

    let ledger_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM _rango_migrations")
        .fetch_one(&pool)
        .await
        .expect("count migration ledger");
    assert_eq!(ledger_rows, 1);

    drop_table(&pool, &project).await;
    project.remove();
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn pure_type_change() {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to postgres");
    let project = Project::new("pure_type_change");
    reset(&pool, &project).await;

    project.write_models(
        r#"
#[derive(Model)]
pub struct Item {
    pub id: FieldUuid,
    pub amount: FieldText,
}
"#,
    );
    assert_success(&project.makemigrations(), "initial makemigrations");
    assert_success(&project.migrate(), "initial migrate");

    let table = project.table();
    sqlx::query(&format!(
        "INSERT INTO \"{}\" (id, amount) VALUES (gen_random_uuid(), $1)",
        table
    ))
    .bind("1")
    .execute(&pool)
    .await
    .expect("insert 1");
    sqlx::query(&format!(
        "INSERT INTO \"{}\" (id, amount) VALUES (gen_random_uuid(), $1)",
        table
    ))
    .bind("2")
    .execute(&pool)
    .await
    .expect("insert 2");

    project.write_models(
        r#"
#[derive(Model)]
pub struct Item {
    pub id: FieldUuid,
    pub amount: FieldInt,
}
"#,
    );
    assert_success(&project.makemigrations(), "v2 makemigrations");

    let sql = project.latest_migration_sql();
    assert!(sql.contains("USING \"amount\"::INTEGER"));
    assert!(!sql.contains("RENAME"));
    assert!(!sql.contains("DROP COLUMN"));

    assert_success(&project.migrate(), "v2 migrate");

    assert_eq!(
        column_type(&pool, &table, "amount").await.as_deref(),
        Some("integer")
    );

    let sum: i64 = sqlx::query_scalar(&format!("SELECT SUM(amount)::bigint FROM \"{}\"", table))
        .fetch_one(&pool)
        .await
        .expect("sum amount");
    assert_eq!(sum, 3);

    drop_table(&pool, &project).await;
    project.remove();
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn rename_only() {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to postgres");
    let project = Project::new("rename_only");
    reset(&pool, &project).await;

    project.write_models(
        r#"
#[derive(Model)]
pub struct Item {
    pub id: FieldUuid,
    pub amount: FieldText,
}
"#,
    );
    assert_success(&project.makemigrations(), "initial makemigrations");
    assert_success(&project.migrate(), "initial migrate");

    let table = project.table();
    sqlx::query(&format!(
        "INSERT INTO \"{}\" (id, amount) VALUES (gen_random_uuid(), $1)",
        table
    ))
    .bind("x")
    .execute(&pool)
    .await
    .expect("insert x");

    project.write_models(
        r#"
#[derive(Model)]
pub struct Item {
    pub id: FieldUuid,
    pub price: FieldText,
}
"#,
    );
    assert_success(&project.makemigrations(), "v2 makemigrations");

    let sql = project.latest_migration_sql();
    assert!(sql.contains("RENAME COLUMN \"amount\" TO \"price\""));
    assert!(!sql.contains("ALTER COLUMN"));
    assert!(!sql.contains("DROP COLUMN"));

    assert_success(&project.migrate(), "v2 migrate");

    let price: String = sqlx::query_scalar(&format!("SELECT price FROM \"{}\"", table))
        .fetch_one(&pool)
        .await
        .expect("select price");
    assert_eq!(price, "x");

    drop_table(&pool, &project).await;
    project.remove();
}

/// Imported files are untrusted: a crafted column name must stay an identifier, never SQL.
#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn import_does_not_inject_column_names() {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to postgres");
    let project = Project::new("import_inject");
    for sql in [
        "DROP TABLE IF EXISTS e2e_import_victim",
        "DROP TABLE IF EXISTS e2e_import_target",
        "CREATE TABLE e2e_import_victim (id INT)",
        "CREATE TABLE e2e_import_target (id INT)",
    ] {
        sqlx::query(sql).execute(&pool).await.expect(sql);
    }

    let evil =
        r#"id") SELECT 1 FROM e2e_import_victim WHERE (SELECT pg_sleep(0)) IS NOT NULL OR "id"#;
    let file = project.dir.join("evil.json");
    std::fs::write(&file, serde_json::json!([{ evil: 1 }]).to_string()).expect("write json");

    let output = project.run_cli(&["import", "evil.json", "-t", "e2e_import_target"]);
    assert_failure(&output, "import with crafted column name");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does not exist"),
        "unexpected error: {stderr}"
    );

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM e2e_import_target")
        .fetch_one(&pool)
        .await
        .expect("count target");
    assert_eq!(rows, 0);

    for sql in [
        "DROP TABLE e2e_import_victim",
        "DROP TABLE e2e_import_target",
    ] {
        sqlx::query(sql).execute(&pool).await.expect(sql);
    }
    project.remove();
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn import_inserts_parents_first_and_is_atomic() {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to postgres");
    let project = Project::new("import_fk");
    for sql in [
        "DROP TABLE IF EXISTS e2e_import_child",
        "DROP TABLE IF EXISTS e2e_import_parent",
        "CREATE TABLE e2e_import_parent (id INT PRIMARY KEY)",
        "CREATE TABLE e2e_import_child (id INT PRIMARY KEY, parent_id INT NOT NULL REFERENCES e2e_import_parent(id))",
    ] {
        sqlx::query(sql).execute(&pool).await.expect(sql);
    }
    let count = |table: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(&format!("SELECT count(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .expect("count rows")
        }
    };

    // Child rows come first in the file.
    let dump = serde_json::json!([
        { "__table": "e2e_import_child", "id": 1, "parent_id": 1 },
        { "__table": "e2e_import_parent", "id": 1 },
    ]);
    std::fs::write(project.dir.join("ok.json"), dump.to_string()).expect("write json");
    let output = project.run_cli(&["import", "ok.json"]);
    assert_success(&output, "import child before parent");
    assert_eq!(count("e2e_import_parent").await, 1);
    assert_eq!(count("e2e_import_child").await, 1);

    // A dangling FK fails the whole import: the valid parent row is rolled back too.
    let dump = serde_json::json!([
        { "__table": "e2e_import_parent", "id": 2 },
        { "__table": "e2e_import_child", "id": 2, "parent_id": 99 },
    ]);
    std::fs::write(project.dir.join("bad.json"), dump.to_string()).expect("write json");
    let output = project.run_cli(&["import", "bad.json"]);
    assert_failure(&output, "import with dangling FK");
    assert_eq!(count("e2e_import_parent").await, 1);
    assert_eq!(count("e2e_import_child").await, 1);

    for sql in [
        "DROP TABLE e2e_import_child",
        "DROP TABLE e2e_import_parent",
    ] {
        sqlx::query(sql).execute(&pool).await.expect(sql);
    }
    project.remove();
}

// ─── dump / restore ───────────────────────────────────────────────────────────

const DUMP_MODELS: &str = r#"
#[derive(Model)]
#[model(table = "e2e_dump_person")]
pub struct Person {
    pub id: FieldUuid,
    pub name: FieldVarchar<1, 100>,
    pub bio: Option<FieldText>,
    pub active: FieldBool,
    pub score: FieldDouble,
    pub price: FieldDecimal<10, 2>,
    pub born: FieldDate,
    pub seen: FieldDateTime,
    pub data: FieldJson,
}

#[derive(Model)]
#[model(table = "e2e_dump_post")]
pub struct Post {
    pub id: FieldUuid,
    pub author: ForeignKey<Person>,
    pub title: FieldText,
}
"#;

const DUMP_TABLES: [&str; 2] = ["e2e_dump_post", "e2e_dump_person"];

async fn drop_dump_tables(pool: &PgPool) {
    for t in DUMP_TABLES {
        sqlx::query(&format!("DROP TABLE IF EXISTS {t} CASCADE"))
            .execute(pool)
            .await
            .expect("drop dump table");
    }
    sqlx::query("DROP TABLE IF EXISTS _rango_migrations")
        .execute(pool)
        .await
        .expect("drop ledger");
}

/// Project with the dump models migrated and two persons + one post inserted.
async fn dump_project(pool: &PgPool, name: &str) -> Project {
    drop_dump_tables(pool).await;
    let project = Project::new(name);
    project.write_models(DUMP_MODELS);
    let out = project.run_cli(&["makemigrations", "src", "-o", "migrations", "--yes"]);
    assert_success(&out, "makemigrations");
    assert_success(&project.migrate(), "migrate");
    for sql in [
        "INSERT INTO e2e_dump_person (id, name, bio, active, score, price, born, seen, data) VALUES \
         ('00000000-0000-0000-0000-000000000001', 'Ann \"q\"', 'hé', true, 1.5, 12.34, '2024-02-29', '2024-05-06 07:08:09.123456+00', '{\"k\":[1,\"é\",null]}'), \
         ('00000000-0000-0000-0000-000000000002', 'Bob', NULL, false, -0.25, 0.01, '1999-12-31', '2000-01-01 00:00:00+00', '{}')",
        "INSERT INTO e2e_dump_post (id, author, title) VALUES \
         ('00000000-0000-0000-0000-0000000000a1', '00000000-0000-0000-0000-000000000001', 'Hello')",
    ] {
        sqlx::query(sql).execute(pool).await.expect(sql);
    }
    project
}

async fn dump_state(pool: &PgPool) -> Vec<String> {
    let mut out = Vec::new();
    for t in ["e2e_dump_person", "e2e_dump_post"] {
        let rows: Vec<String> =
            sqlx::query_scalar(&format!("SELECT to_jsonb(t)::text FROM {t} t ORDER BY id"))
                .fetch_all(pool)
                .await
                .expect("select rows");
        out.extend(rows);
    }
    out
}

async fn dump_count(pool: &PgPool, table: &str) -> Option<i64> {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .ok()
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn dump_restore_round_trip() {
    let pool = PgPool::connect(&database_url()).await.expect("connect");
    let project = dump_project(&pool, "dump_rt").await;
    let before = dump_state(&pool).await;

    assert_success(&project.run_cli(&["dump", "-o", "d.json"]), "dump");
    let dump: serde_json::Value =
        serde_json::from_slice(&std::fs::read(project.dir.join("d.json")).unwrap()).unwrap();
    let names: Vec<&str> = dump["tables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["e2e_dump_person", "e2e_dump_post"]);

    // Restoring over existing data fails and changes nothing.
    let out = project.run_cli(&["restore", "d.json"]);
    assert_failure(&out, "restore over existing rows");
    assert!(String::from_utf8_lossy(&out.stderr).contains("e2e_dump_person"));
    assert_eq!(dump_state(&pool).await, before);

    drop_dump_tables(&pool).await;
    assert_success(&project.run_cli(&["restore", "d.json"]), "restore");
    assert_eq!(dump_state(&pool).await, before);

    // A backup is restorable too.
    assert_success(&project.run_cli(&["backup", "--dir", "bk"]), "backup");
    let file = std::fs::read_dir(project.dir.join("bk"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    drop_dump_tables(&pool).await;
    let path = file.path();
    assert_success(
        &project.run_cli(&["restore", path.to_str().unwrap()]),
        "restore backup",
    );
    assert_eq!(dump_state(&pool).await, before);

    drop_dump_tables(&pool).await;
    project.remove();
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn dump_schema_only_restores_empty_tables() {
    let pool = PgPool::connect(&database_url()).await.expect("connect");
    let project = dump_project(&pool, "dump_schema").await;
    assert_success(
        &project.run_cli(&["dump", "--schema-only", "-o", "s.json"]),
        "dump",
    );
    drop_dump_tables(&pool).await;
    assert_success(&project.run_cli(&["restore", "s.json"]), "restore");
    assert_eq!(dump_count(&pool, "e2e_dump_person").await, Some(0));
    assert_eq!(dump_count(&pool, "e2e_dump_post").await, Some(0));
    drop_dump_tables(&pool).await;
    project.remove();
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn dump_restore_is_all_or_nothing() {
    let pool = PgPool::connect(&database_url()).await.expect("connect");
    let project = dump_project(&pool, "dump_atomic").await;
    assert_success(&project.run_cli(&["dump", "-o", "d.json"]), "dump");
    // Dangling FK in the last table: persons and the schema must roll back.
    let mut dump: serde_json::Value =
        serde_json::from_slice(&std::fs::read(project.dir.join("d.json")).unwrap()).unwrap();
    dump["tables"][1]["rows"][0]["author"] =
        serde_json::json!("00000000-0000-0000-0000-0000000000ff");
    std::fs::write(project.dir.join("bad.json"), dump.to_string()).unwrap();

    drop_dump_tables(&pool).await;
    assert_failure(&project.run_cli(&["restore", "bad.json"]), "restore");
    assert_eq!(dump_count(&pool, "e2e_dump_person").await, None);
    assert_eq!(dump_count(&pool, "e2e_dump_post").await, None);
    project.remove();
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn export_import_round_trip_typed() {
    let pool = PgPool::connect(&database_url()).await.expect("connect");
    let project = dump_project(&pool, "dump_exp").await;
    let before = dump_state(&pool).await;
    assert_success(&project.run_cli(&["export", "-o", "e.json"]), "export");
    for t in DUMP_TABLES {
        sqlx::query(&format!("DELETE FROM {t}"))
            .execute(&pool)
            .await
            .unwrap();
    }
    assert_success(&project.run_cli(&["import", "e.json"]), "import");
    assert_eq!(dump_state(&pool).await, before);
    // Every row now conflicts on its primary key: --replace updates them in place.
    assert_success(
        &project.run_cli(&["import", "e.json", "--replace"]),
        "import --replace",
    );
    assert_eq!(dump_state(&pool).await, before);
    drop_dump_tables(&pool).await;
    project.remove();
}

async fn drop_named(pool: &PgPool, tables: &[&str]) {
    for t in tables {
        sqlx::query(&format!("DROP TABLE IF EXISTS \"{t}\" CASCADE"))
            .execute(pool)
            .await
            .expect("drop table");
    }
    sqlx::query("DROP TABLE IF EXISTS _rango_migrations")
        .execute(pool)
        .await
        .expect("drop migration ledger");
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn explicit_table_name_is_used_verbatim() {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to postgres");
    let project = Project::new("verbatim");
    drop_named(&pool, &["users"]).await;

    project.write_models(
        r#"
#[derive(Model)]
#[model(table = "users")]
pub struct User {
    pub id: FieldUuid,
    pub name: FieldText,
}
"#,
    );
    assert_success(&project.makemigrations(), "makemigrations");
    assert_success(&project.migrate(), "migrate");
    assert!(column_type(&pool, "users", "name").await.is_some());

    drop_named(&pool, &["users"]).await;
    project.remove();
}

#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn circular_foreign_keys_migrate() {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to postgres");
    let project = Project::new("cycle");
    drop_named(&pool, &["e2e_cycle_projects", "e2e_cycle_generations"]).await;

    project.write_models(
        r#"
#[derive(Model)]
#[model(table = "e2e_cycle_projects")]
pub struct Project {
    pub id: FieldUuid,
    pub identity: Option<ForeignKey<Generation>>,
}

#[derive(Model)]
#[model(table = "e2e_cycle_generations")]
pub struct Generation {
    pub id: FieldUuid,
    pub project: ForeignKey<Project>,
}
"#,
    );
    assert_success(&project.makemigrations(), "makemigrations");
    assert!(project.latest_migration_sql().contains("ADD CONSTRAINT"));
    assert_success(&project.migrate(), "migrate");

    // Snapshot keeps the references: a second run sees no change.
    let again = project.makemigrations();
    assert_success(&again, "second makemigrations");
    assert_eq!(project.migration_sql_files().len(), 1);

    drop_named(&pool, &["e2e_cycle_projects", "e2e_cycle_generations"]).await;
    project.remove();
}

/// Every FK of the cycle is NOT NULL: the rows can only be loaded with the FKs deferred, both
/// by hand and by `restore`.
#[tokio::test]
#[cfg_attr(not(feature = "integration"), ignore)]
async fn not_null_foreign_key_cycle_restores() {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to postgres");
    let project = Project::new("ring");
    let tables = ["e2e_ring_a", "e2e_ring_b"];
    drop_named(&pool, &tables).await;

    project.write_models(
        r#"
#[derive(Model)]
#[model(table = "e2e_ring_a")]
pub struct RingA {
    pub id: FieldUuid,
    pub b: ForeignKey<RingB>,
}

#[derive(Model)]
#[model(table = "e2e_ring_b")]
pub struct RingB {
    pub id: FieldUuid,
    pub a: ForeignKey<RingA>,
}
"#,
    );
    assert_success(&project.makemigrations(), "makemigrations");
    assert!(
        project
            .latest_migration_sql()
            .contains("DEFERRABLE INITIALLY IMMEDIATE")
    );
    assert_success(&project.migrate(), "migrate");

    // Outside a transaction that defers them, the constraints are checked per statement.
    let (a, b) = (
        "00000000-0000-0000-0000-00000000000a",
        "00000000-0000-0000-0000-00000000000b",
    );
    assert!(
        sqlx::query(&format!(
            "INSERT INTO e2e_ring_a (id, b) VALUES ('{a}', '{b}')"
        ))
        .execute(&pool)
        .await
        .is_err()
    );
    let mut tx = pool.begin().await.unwrap();
    for sql in [
        "SET CONSTRAINTS ALL DEFERRED".to_string(),
        format!("INSERT INTO e2e_ring_a (id, b) VALUES ('{a}', '{b}')"),
        format!("INSERT INTO e2e_ring_b (id, a) VALUES ('{b}', '{a}')"),
    ] {
        sqlx::query(&sql).execute(&mut *tx).await.expect(&sql);
    }
    tx.commit().await.expect("commit the cycle");

    assert_success(
        &project.run_cli(&[
            "dump",
            "-o",
            "ring.json",
            "-t",
            "e2e_ring_a",
            "-t",
            "e2e_ring_b",
        ]),
        "dump",
    );
    drop_named(&pool, &tables).await;
    assert_success(&project.run_cli(&["restore", "ring.json"]), "restore");
    for t in tables {
        assert_eq!(dump_count(&pool, t).await, Some(1), "{t}");
    }

    drop_named(&pool, &tables).await;
    project.remove();
}
