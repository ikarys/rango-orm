//! End-to-end `dump` / `restore` / `backup` tests against SQLite (no server needed).

use sqlx::{Connection, SqliteConnection};
use std::path::PathBuf;
use std::process::Output;

struct Project {
    dir: PathBuf,
}

impl Project {
    /// A project with two models (`Author`, `Book` -> `Author`) migrated into `a.db`.
    async fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rango_dump_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("rango.toml"), "[database]\nbackend = \"sqlite\"\n").unwrap();
        std::fs::write(
            dir.join("src/models.rs"),
            r#"use rango::prelude::*;

#[derive(Model)]
pub struct Author {
    pub id: FieldUuid,
    pub name: FieldText,
}

#[derive(Model)]
pub struct Book {
    pub id: FieldUuid,
    pub title: FieldText,
    pub author: ForeignKey<Author>,
}
"#,
        )
        .unwrap();
        let p = Self { dir };
        assert_ok(&p.cli("a.db", &["makemigrations", "src", "-p", "t", "--yes"]));
        assert_ok(&p.cli("a.db", &["migrate"]));
        let mut c = p.conn("a.db").await;
        for sql in [
            "INSERT INTO t_author (id, name) VALUES ('a1', 'Ada'), ('a2', 'Bob')",
            "INSERT INTO t_book (id, title, author) VALUES ('b1', 'Notes', 'a1')",
        ] {
            sqlx::query(sql).execute(&mut c).await.unwrap();
        }
        p
    }

    fn url(&self, db: &str) -> String {
        format!("sqlite://{}?mode=rwc", self.dir.join(db).display())
    }

    async fn conn(&self, db: &str) -> SqliteConnection {
        SqliteConnection::connect(&self.url(db)).await.unwrap()
    }

    fn cli(&self, db: &str, args: &[&str]) -> Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_rango"))
            .current_dir(&self.dir)
            .env("DATABASE_URL", self.url(db))
            .stdin(std::process::Stdio::null())
            .args(args)
            .output()
            .unwrap()
    }

    fn write(&self, name: &str, content: &str) -> String {
        std::fs::write(self.dir.join(name), content).unwrap();
        name.to_string()
    }

    /// Row count, or `None` when the table does not exist.
    async fn count(&self, db: &str, table: &str) -> Option<i64> {
        let mut c = self.conn(db).await;
        sqlx::query_scalar(&format!("SELECT count(*) FROM \"{table}\""))
            .fetch_one(&mut c)
            .await
            .ok()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn assert_ok(o: &Output) {
    assert!(
        o.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}

fn failure_stderr(o: &Output) -> String {
    assert!(!o.status.success(), "unexpected success");
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[cfg(unix)]
fn assert_mode_600(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[tokio::test]
async fn dump_then_restore_round_trips_rows() {
    let p = Project::new("roundtrip").await;
    assert_ok(&p.cli("a.db", &["dump", "-o", "d.json"]));
    #[cfg(unix)]
    assert_mode_600(&p.dir.join("d.json"));

    let dump: serde_json::Value =
        serde_json::from_slice(&std::fs::read(p.dir.join("d.json")).unwrap()).unwrap();
    assert_eq!(dump["format"], "rango-dump");
    assert_eq!(dump["version"], 1);
    assert_eq!(dump["backend"], "sqlite");
    let names: Vec<&str> = dump["tables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["t_author", "t_book"], "parents first, no ledger");

    assert_ok(&p.cli("b.db", &["restore", "d.json"]));
    assert_eq!(p.count("b.db", "t_author").await, Some(2));
    assert_eq!(p.count("b.db", "t_book").await, Some(1));
    let mut c = p.conn("b.db").await;
    let title: String = sqlx::query_scalar("SELECT title FROM t_book WHERE author = 'a1'")
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert_eq!(title, "Notes");
}

#[tokio::test]
async fn schema_only_creates_empty_tables() {
    let p = Project::new("schema_only").await;
    assert_ok(&p.cli("a.db", &["dump", "--schema-only", "-o", "s.json"]));
    assert_ok(&p.cli("b.db", &["restore", "s.json"]));
    assert_eq!(p.count("b.db", "t_author").await, Some(0));
    assert_eq!(p.count("b.db", "t_book").await, Some(0));
}

#[tokio::test]
async fn backup_is_gzip_private_and_restorable() {
    let p = Project::new("backup").await;
    assert_ok(&p.cli("a.db", &["backup", "--dir", "bk"]));
    let files: Vec<PathBuf> = std::fs::read_dir(p.dir.join("bk"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("rango-backup-") && name.ends_with(".json.gz"),
        "{name}"
    );
    assert_eq!(&std::fs::read(&files[0]).unwrap()[..2], [0x1f, 0x8b]);
    #[cfg(unix)]
    assert_mode_600(&files[0]);

    // Detected by magic bytes, not by extension.
    std::fs::rename(&files[0], p.dir.join("backup.bin")).unwrap();
    assert_ok(&p.cli("b.db", &["restore", "backup.bin"]));
    assert_eq!(p.count("b.db", "t_author").await, Some(2));
    assert_eq!(p.count("b.db", "t_book").await, Some(1));
}

#[tokio::test]
async fn restore_rejects_foreign_json_and_unknown_version() {
    let p = Project::new("reject").await;
    for (file, body, needle) in [
        ("arr.json", r#"[{"id": 1}]"#, "Not a Rango dump"),
        (
            "fmt.json",
            r#"{"format": "pg_dump", "version": 1}"#,
            "Not a Rango dump",
        ),
        (
            "ver.json",
            r#"{"format": "rango-dump", "version": 99, "tables": []}"#,
            "Unsupported dump version 99",
        ),
    ] {
        let f = p.write(file, body);
        let err = failure_stderr(&p.cli("b.db", &["restore", &f]));
        assert!(err.contains(needle), "{file}: {err}");
    }
}

fn dump_json(schema: &str, table: &str, column: &str) -> String {
    format!(
        r#"{{"format": "rango-dump", "version": 1, "rango_version": "x", "backend": "sqlite",
            "created_at": "x", "schema": {schema},
            "tables": [{{"name": {t}, "rows": [{{{c}: "x"}}]}}]}}"#,
        t = serde_json::to_string(table).unwrap(),
        c = serde_json::to_string(column).unwrap(),
    )
}

#[tokio::test]
async fn malicious_names_never_run_injected_sql() {
    let p = Project::new("evil").await;
    // Minimal schema; the Snapshot JSON shape is taken from a real dump below.
    assert_ok(&p.cli("a.db", &["dump", "--schema-only", "-o", "real.json"]));
    let real: serde_json::Value =
        serde_json::from_slice(&std::fs::read(p.dir.join("real.json")).unwrap()).unwrap();
    let schema = real["schema"].to_string();

    for (schema, table, column) in [
        ("null", "t_author\"; DROP TABLE t_book; --", "name"),
        (
            "null",
            "t_author",
            "name\") VALUES ('x'); DROP TABLE t_book; --",
        ),
        ("null", "t_author", "na`me;--"),
        (&schema, "t_author\"; DROP TABLE t_book; --", "id"),
        (&schema, "t_author", "id\"; DROP TABLE t_book; --"),
    ] {
        let f = p.write("evil.json", &dump_json(schema, table, column));
        assert!(
            !p.cli("a.db", &["restore", &f]).status.success(),
            "{table} / {column}"
        );
        assert_eq!(
            p.count("a.db", "t_book").await,
            Some(1),
            "{table} / {column}"
        );
        assert_eq!(p.count("a.db", "t_author").await, Some(2));
    }

    // Raw SQL fragments of the schema (defaults) must stay a single statement.
    let mut evil = real.clone();
    evil["schema"]["tables"]["t_author"]["columns"][1]["default"] =
        serde_json::json!("'x'); DROP TABLE t_book; --");
    evil["tables"] = serde_json::json!([]);
    let f = p.write("evil2.json", &evil.to_string());
    let err = failure_stderr(&p.cli("b.db", &["restore", &f]));
    assert!(!err.is_empty());
    assert_eq!(p.count("b.db", "t_author").await, None);
    assert_eq!(p.count("a.db", "t_book").await, Some(1));
}

#[tokio::test]
async fn restore_is_all_or_nothing() {
    let p = Project::new("atomic").await;
    // The second row violates NOT NULL: the first (valid) row must roll back.
    let dump = r#"{"format": "rango-dump", "version": 1, "rango_version": "x",
        "backend": "sqlite", "created_at": "x", "schema": null,
        "tables": [{"name": "t_author", "rows": [
            {"id": "n1", "name": "New"}, {"id": "n2", "name": null}]}]}"#;
    let f = p.write("bad.json", dump);
    assert!(!p.cli("a.db", &["restore", &f]).status.success());
    assert_eq!(p.count("a.db", "t_author").await, Some(2));

    // With a schema into a fresh database, the failed restore leaves no tables behind.
    assert_ok(&p.cli("a.db", &["dump", "--schema-only", "-o", "s.json"]));
    let mut d: serde_json::Value =
        serde_json::from_slice(&std::fs::read(p.dir.join("s.json")).unwrap()).unwrap();
    d["tables"] = serde_json::json!([{"name": "t_author", "rows": [
        {"id": "n1", "name": "New"}, {"id": "n2", "name": null}]}]);
    let f = p.write("bad2.json", &d.to_string());
    assert!(!p.cli("b.db", &["restore", &f]).status.success());
    assert_eq!(p.count("b.db", "t_author").await, None);
}

#[tokio::test]
async fn restore_fails_on_conflict_instead_of_dropping_rows() {
    let p = Project::new("conflict").await;
    assert_ok(&p.cli("a.db", &["dump", "-o", "d.json"]));
    // Restoring into the database it came from collides on every primary key.
    let err = failure_stderr(&p.cli("a.db", &["restore", "d.json"]));
    assert!(err.contains("t_author"), "{err}");
    assert_eq!(p.count("a.db", "t_author").await, Some(2));
    assert_eq!(p.count("a.db", "t_book").await, Some(1));
}
