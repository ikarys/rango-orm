//! `export --format csv` then `import` round trip against SQLite (no server needed).

use sqlx::{Connection, Row, SqliteConnection};
use std::path::Path;

const SCHEMA: &str =
    "CREATE TABLE t_user (zeta TEXT, id INTEGER PRIMARY KEY, meta TEXT, note TEXT)";

fn url(dir: &Path, db: &str) -> String {
    format!("sqlite://{}?mode=rwc", dir.join(db).display())
}

fn cli(dir: &Path, db: &str, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_rango"))
        .current_dir(dir)
        .env("DATABASE_URL", url(dir, db))
        .stdin(std::process::Stdio::null())
        .args(args)
        .output()
        .unwrap()
}

async fn exec(dir: &Path, db: &str, sql: &str) {
    let mut c = SqliteConnection::connect(&url(dir, db)).await.unwrap();
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(&mut c)
        .await
        .unwrap();
}

#[tokio::test]
async fn csv_export_import_round_trip() {
    let dir = std::env::temp_dir().join(format!("rango_csv_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    exec(&dir, "a.db", SCHEMA).await;
    exec(
        &dir,
        "a.db",
        "INSERT INTO t_user VALUES ('Smith, \"Ann\"', 1, '{\"bulk\":true,\"plan\":\"pro\"}', 'line1\nline2'), \
         ('z', 2, NULL, '')",
    )
    .await;

    let out = cli(
        &dir,
        "a.db",
        &["export", "--table", "t_user", "--format", "csv"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let csv = String::from_utf8(out.stdout).unwrap();
    // Table column order, not alphabetical.
    assert!(csv.starts_with("zeta,id,meta,note\n"), "{csv}");
    std::fs::write(dir.join("u.csv"), &csv).unwrap();

    let out = cli(&dir, "a.db", &["export", "--format", "csv"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("CSV export needs --table"));

    exec(&dir, "b.db", SCHEMA).await;
    let out = cli(&dir, "b.db", &["import", "u.csv", "--table", "t_user"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mut c = SqliteConnection::connect(&url(&dir, "b.db")).await.unwrap();
    let rows = sqlx::query("SELECT zeta, meta, note FROM t_user ORDER BY id")
        .fetch_all(&mut c)
        .await
        .unwrap();
    assert_eq!(rows[0].get::<String, _>(0), "Smith, \"Ann\"");
    assert_eq!(
        rows[0].get::<String, _>(1),
        "{\"bulk\":true,\"plan\":\"pro\"}"
    );
    assert_eq!(rows[0].get::<String, _>(2), "line1\nline2");
    assert_eq!(rows[1].get::<Option<String>, _>(1), None);
    assert_eq!(rows[1].get::<String, _>(2), "");

    let _ = std::fs::remove_dir_all(&dir);
}
