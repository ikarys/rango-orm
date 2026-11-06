//! End-to-end migration tests for the `rango` CLI against MySQL/MariaDB.
//!
//! Run with: `MYSQL_URL=mysql://root:rango@127.0.0.1:3307/rango_test cargo test -p rango-orm-cli --test migrations_e2e_mysql --features integration-mysql -- --test-threads=1`

use std::path::PathBuf;

use sqlx::MySqlPool;

fn database_url() -> String {
    std::env::var("MYSQL_URL")
        .unwrap_or_else(|_| "mysql://root:rango@127.0.0.1:3307/rango_test".into())
}

struct Project {
    dir: PathBuf,
    prefix: String,
}

impl Project {
    fn new(name: &str) -> Self {
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("rango_e2e_mysql_{name}_{pid}"));
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        std::fs::create_dir_all(dir.join("src")).expect("create project src dir");
        std::fs::write(dir.join("rango.toml"), "[database]\nbackend = \"mysql\"\n")
            .expect("write rango.toml");

        Self {
            dir,
            prefix: format!("e2e_my_{name}"),
        }
    }

    fn write_models(&self, body: &str) {
        let content = format!("use rango::prelude::*;\n\n{body}");
        std::fs::write(self.dir.join("src/models.rs"), content).expect("write models.rs");
    }

    fn makemigrations(&self) -> std::process::Output {
        self.run_cli(&[
            "makemigrations",
            "src",
            "-o",
            "migrations",
            "-p",
            &self.prefix,
            "--yes",
        ])
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

    fn remove(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn drop_tables(pool: &MySqlPool, tables: &[&str]) {
    let mut sql = String::from("SET FOREIGN_KEY_CHECKS = 0; ");
    for table in tables {
        sql.push_str(&format!("DROP TABLE IF EXISTS `{table}`; "));
    }
    sql.push_str("SET FOREIGN_KEY_CHECKS = 1;");
    sqlx::raw_sql(&sql)
        .execute(pool)
        .await
        .expect("drop tables");
}

async fn column_type(pool: &MySqlPool, table: &str, column: &str) -> Option<String> {
    let sql = "SELECT CAST(COLUMN_TYPE AS CHAR) FROM information_schema.COLUMNS \
              WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND COLUMN_NAME = ?";
    sqlx::query_scalar::<_, String>(sql)
        .bind(table)
        .bind(column)
        .fetch_optional(pool)
        .await
        .expect("query column type")
        .map(|value| value.to_lowercase())
}

async fn is_nullable(pool: &MySqlPool, table: &str, column: &str) -> bool {
    let sql = "SELECT IS_NULLABLE = 'YES' FROM information_schema.COLUMNS \
              WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND COLUMN_NAME = ?";
    sqlx::query_scalar::<_, i64>(sql)
        .bind(table)
        .bind(column)
        .fetch_optional(pool)
        .await
        .expect("query nullability")
        .is_some_and(|value| value == 1)
}

async fn index_names(pool: &MySqlPool, table: &str) -> Vec<String> {
    let sql = "SELECT DISTINCT CAST(INDEX_NAME AS CHAR) FROM information_schema.STATISTICS \
              WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?";
    sqlx::query_scalar::<_, String>(sql)
        .bind(table)
        .fetch_all(pool)
        .await
        .expect("query indexes")
}

async fn constraint_names(pool: &MySqlPool, table: &str, kind: &str) -> Vec<String> {
    let sql = "SELECT CAST(CONSTRAINT_NAME AS CHAR) FROM information_schema.TABLE_CONSTRAINTS \
              WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND CONSTRAINT_TYPE = ?";
    sqlx::query_scalar::<_, String>(sql)
        .bind(table)
        .bind(kind)
        .fetch_all(pool)
        .await
        .expect("query constraints")
}

async fn count_rows(pool: &MySqlPool, table: &str) -> i64 {
    let sql = format!("SELECT count(*) FROM `{table}`");
    sqlx::query_scalar::<_, i64>(&sql)
        .fetch_one(pool)
        .await
        .expect("count rows")
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
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn lifecycle_create_then_alter() {
    let pool = MySqlPool::connect(&database_url())
        .await
        .expect("connect to mysql");
    let project = Project::new("life");
    drop_tables(
        &pool,
        &[
            "e2e_my_life_book_e2e_my_life_author",
            "e2e_my_life_book",
            "e2e_my_life_author",
            "_rango_migrations",
        ],
    )
    .await;

    project.write_models(
        r#"
#[derive(Model)]
#[model(constraints = [CheckConstraint::new("age >= 0").name("chk_age"), UniqueConstraint::on(&["name", "age"]).name("uniq_name_age")])]
pub struct Author {
    pub id: FieldUuid,
    #[field(unique)]
    pub email: FieldVarchar<1, 200>,
    pub name: FieldVarchar<1, 100>,
    pub age: FieldInt,
    pub bio: Option<FieldText>,
    pub data: FieldJson,
}

#[derive(Model)]
pub struct Book {
    pub id: FieldUuid,
    pub author: ForeignKey<Author>,
    #[field(index)]
    pub title: FieldVarchar<1, 200>,
    pub readers: ManyToMany<Author>,
}
"#,
    );

    assert_success(&project.makemigrations(), "initial makemigrations");
    let sql = project.latest_migration_sql();
    assert!(sql.contains("`e2e_my_life_author`"));
    assert!(sql.contains("ENGINE=InnoDB"));
    assert_success(&project.migrate(), "initial migrate");

    let second = project.migrate();
    assert_success(&second, "second migrate");
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(stdout.contains("Nothing to apply"));

    let author = "e2e_my_life_author";
    let book = "e2e_my_life_book";
    let pivot = "e2e_my_life_book_e2e_my_life_author";

    assert_eq!(
        column_type(&pool, author, "id").await.as_deref(),
        Some("binary(16)")
    );
    let age_type = column_type(&pool, author, "age")
        .await
        .expect("age column type");
    assert!(age_type.starts_with("int"), "age type: {age_type}");

    let foreign_keys = constraint_names(&pool, book, "FOREIGN KEY").await;
    assert!(
        foreign_keys
            .iter()
            .any(|name| name == "e2e_my_life_book_author_fkey"),
        "missing FK constraint: {foreign_keys:?}"
    );

    let uniques = constraint_names(&pool, author, "UNIQUE").await;
    assert!(
        uniques
            .iter()
            .any(|name| name == "e2e_my_life_author_email_unique"),
        "missing email unique constraint: {uniques:?}"
    );
    assert!(
        uniques.iter().any(|name| name == "uniq_name_age"),
        "missing name/age unique constraint: {uniques:?}"
    );

    let book_indexes = index_names(&pool, book).await;
    assert!(
        book_indexes
            .iter()
            .any(|name| name == "e2e_my_life_book_title_idx"),
        "missing title index: {book_indexes:?}"
    );

    assert_eq!(
        column_type(&pool, pivot, "e2e_my_life_book_id")
            .await
            .as_deref(),
        Some("binary(16)")
    );

    let insert = sqlx::query(
        "INSERT INTO e2e_my_life_author (id, email, name, age, data) \
         VALUES (?, 'x@y.z', 'X', -1, '{}')",
    )
    .bind(vec![1u8; 16])
    .execute(&pool)
    .await;
    assert!(
        insert.is_err(),
        "insert violating CHECK unexpectedly succeeded"
    );

    project.write_models(
        r#"
#[derive(Model)]
#[model(constraints = [UniqueConstraint::on(&["name", "age"]).name("uniq_name_age")])]
pub struct Author {
    pub id: FieldUuid,
    pub email: FieldVarchar<1, 200>,
    #[field(index)]
    pub name: FieldVarchar<1, 150>,
    pub age: FieldBigInt,
    pub bio: Option<FieldText>,
    pub data: FieldJson,
}

#[derive(Model)]
pub struct Book {
    pub id: FieldUuid,
    pub author: Option<ForeignKey<Author>>,
    pub title: FieldVarchar<1, 200>,
    pub readers: ManyToMany<Author>,
}
"#,
    );

    assert_success(&project.makemigrations(), "v2 makemigrations");
    let sql = project.latest_migration_sql();
    assert!(sql.contains("MODIFY COLUMN"));
    assert!(sql.contains("DROP CONSTRAINT `chk_age`"));
    assert_success(&project.migrate(), "v2 migrate");

    let name_type = column_type(&pool, author, "name")
        .await
        .expect("name column type");
    assert_eq!(name_type, "varchar(150)");

    let age_type = column_type(&pool, author, "age")
        .await
        .expect("age column type");
    assert!(age_type.starts_with("bigint"), "age type: {age_type}");

    assert!(
        is_nullable(&pool, book, "author").await,
        "author should be nullable"
    );

    let foreign_keys = constraint_names(&pool, book, "FOREIGN KEY").await;
    assert!(
        foreign_keys
            .iter()
            .any(|name| name == "e2e_my_life_book_author_fkey"),
        "missing FK constraint after alter: {foreign_keys:?}"
    );

    let uniques = constraint_names(&pool, author, "UNIQUE").await;
    assert!(
        !uniques
            .iter()
            .any(|name| name == "e2e_my_life_author_email_unique"),
        "email unique constraint should be dropped: {uniques:?}"
    );

    let author_indexes = index_names(&pool, author).await;
    assert!(
        author_indexes
            .iter()
            .any(|name| name == "e2e_my_life_author_name_idx"),
        "missing name index: {author_indexes:?}"
    );

    let book_indexes = index_names(&pool, book).await;
    assert!(
        !book_indexes
            .iter()
            .any(|name| name == "e2e_my_life_book_title_idx"),
        "title index should be dropped: {book_indexes:?}"
    );

    let checks = constraint_names(&pool, author, "CHECK").await;
    assert!(
        !checks.iter().any(|name| name == "chk_age"),
        "check constraint should be dropped: {checks:?}"
    );

    drop_tables(
        &pool,
        &[
            "e2e_my_life_book_e2e_my_life_author",
            "e2e_my_life_book",
            "e2e_my_life_author",
            "_rango_migrations",
        ],
    )
    .await;
    project.remove();
}

#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn export_import_round_trip() {
    let pool = MySqlPool::connect(&database_url())
        .await
        .expect("connect to mysql");
    let project = Project::new("rt");
    drop_tables(
        &pool,
        &[
            "e2e_my_rt_post_e2e_my_rt_person",
            "e2e_my_rt_post",
            "e2e_my_rt_person",
            "_rango_migrations",
        ],
    )
    .await;

    project.write_models(
        r#"
#[derive(Model)]
pub struct Person {
    pub id: FieldUuid,
    pub name: FieldVarchar<1, 100>,
    pub bio: Option<FieldText>,
    pub data: FieldJson,
}

#[derive(Model)]
pub struct Post {
    pub id: FieldUuid,
    pub author: ForeignKey<Person>,
    pub tags: ManyToMany<Person>,
}
"#,
    );

    assert_success(&project.makemigrations(), "initial makemigrations");
    assert_success(&project.migrate(), "initial migrate");

    let person_id = vec![0x01u8; 16];
    let post_id = vec![0x02u8; 16];

    sqlx::query(
        "INSERT INTO e2e_my_rt_person (id, name, bio, data) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(&person_id)
    .bind("Ann")
    .bind("hé \"x\"")
    .bind(r#"{"k":[1,"é"]}"#)
    .execute(&pool)
    .await
    .expect("insert person");

    sqlx::query("INSERT INTO e2e_my_rt_post (id, author) VALUES (?, ?)")
        .bind(&post_id)
        .bind(&person_id)
        .execute(&pool)
        .await
        .expect("insert post");

    sqlx::query(
        "INSERT INTO e2e_my_rt_post_e2e_my_rt_person \
         (e2e_my_rt_post_id, e2e_my_rt_person_id) VALUES (?, ?)",
    )
    .bind(&post_id)
    .bind(&person_id)
    .execute(&pool)
    .await
    .expect("insert pivot row");

    let output = project.run_cli(&["export", "-o", "dump.json"]);
    assert_success(&output, "export");

    let dump_path = project.dir.join("dump.json");
    let dump_text = std::fs::read_to_string(&dump_path).expect("read dump.json");
    let dump: Vec<serde_json::Value> = serde_json::from_str(&dump_text).expect("parse dump.json");

    let person_obj = dump
        .iter()
        .find(|value| {
            value.get("__table").and_then(|table| table.as_str()) == Some("e2e_my_rt_person")
        })
        .expect("person object in dump");

    assert_eq!(
        person_obj.get("id").and_then(|value| value.as_str()),
        Some("01010101-0101-0101-0101-010101010101")
    );
    assert_eq!(
        person_obj.get("bio").and_then(|value| value.as_str()),
        Some("hé \"x\"")
    );
    assert_eq!(
        person_obj.get("data"),
        Some(&serde_json::json!({"k": [1, "é"]}))
    );

    sqlx::query("DELETE FROM e2e_my_rt_person")
        .execute(&pool)
        .await
        .expect("delete person");

    let output = project.run_cli(&["import", "dump.json"]);
    assert_success(&output, "import");

    assert_eq!(count_rows(&pool, "e2e_my_rt_person").await, 1);
    assert_eq!(count_rows(&pool, "e2e_my_rt_post").await, 1);
    assert_eq!(
        count_rows(&pool, "e2e_my_rt_post_e2e_my_rt_person").await,
        1
    );

    let bio: String = sqlx::query_scalar("SELECT CAST(bio AS CHAR) FROM e2e_my_rt_person")
        .fetch_one(&pool)
        .await
        .expect("select bio");
    assert_eq!(bio, "hé \"x\"");

    let hex_id: String = sqlx::query_scalar("SELECT HEX(id) FROM e2e_my_rt_person")
        .fetch_one(&pool)
        .await
        .expect("select hex id");
    assert_eq!(hex_id, "01010101010101010101010101010101");

    let output = project.run_cli(&["import", "dump.json"]);
    assert_success(&output, "import again");
    assert_eq!(count_rows(&pool, "e2e_my_rt_person").await, 1);
    assert_eq!(count_rows(&pool, "e2e_my_rt_post").await, 1);
    assert_eq!(
        count_rows(&pool, "e2e_my_rt_post_e2e_my_rt_person").await,
        1
    );

    let output = project.run_cli(&["import", "--replace", "dump.json"]);
    assert_success(&output, "import replace");
    assert_eq!(count_rows(&pool, "e2e_my_rt_person").await, 1);
    assert_eq!(count_rows(&pool, "e2e_my_rt_post").await, 1);
    assert_eq!(
        count_rows(&pool, "e2e_my_rt_post_e2e_my_rt_person").await,
        1
    );

    drop_tables(
        &pool,
        &[
            "e2e_my_rt_post_e2e_my_rt_person",
            "e2e_my_rt_post",
            "e2e_my_rt_person",
            "_rango_migrations",
        ],
    )
    .await;
    project.remove();
}

/// Imported files are untrusted: a crafted column name must stay an identifier, never SQL.
#[tokio::test]
#[cfg_attr(not(feature = "integration-mysql"), ignore)]
async fn import_does_not_inject_column_names() {
    let pool = MySqlPool::connect(&database_url())
        .await
        .expect("connect to mysql");
    let project = Project::new("import_inject");
    drop_tables(
        &pool,
        &[
            "e2e_my_import_victim",
            "e2e_my_import_target",
            "_rango_migrations",
        ],
    )
    .await;

    sqlx::query("CREATE TABLE e2e_my_import_victim (id INT)")
        .execute(&pool)
        .await
        .expect("create victim table");
    sqlx::query("CREATE TABLE e2e_my_import_target (id INT)")
        .execute(&pool)
        .await
        .expect("create target table");

    let evil =
        r#"`id`) SELECT 1 FROM e2e_my_import_victim WHERE (SELECT SLEEP(0)) IS NOT NULL OR (`id`"#;
    let mut row = serde_json::Map::new();
    row.insert(evil.to_string(), serde_json::json!(1));

    let file = project.dir.join("evil.json");
    std::fs::write(&file, serde_json::json!([row]).to_string()).expect("write json");

    let output = project.run_cli(&["import", "evil.json", "-t", "e2e_my_import_target"]);
    assert_failure(&output, "import with crafted column name");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Unknown column"),
        "unexpected error: {stderr}"
    );

    assert_eq!(count_rows(&pool, "e2e_my_import_target").await, 0);

    drop_tables(&pool, &["e2e_my_import_victim", "e2e_my_import_target"]).await;
    project.remove();
}
