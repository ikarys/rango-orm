//! Shared helpers: a throwaway project directory that holds the fixture models
//! (`tests/fixtures/models.rs`) and is driven through the real `rango` binary.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Output;

const FIXTURE: &str = include_str!("../fixtures/models.rs");

pub struct Project {
    pub dir: PathBuf,
}

impl Project {
    /// A project whose `src/models.rs` is the fixture, configured for `backend`
    /// (`sqlite`, `postgres` or `mysql`).
    pub fn new(name: &str, backend: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rango_rp_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("rango.toml"),
            format!("[database]\nbackend = \"{backend}\"\n"),
        )
        .unwrap();
        std::fs::write(dir.join("src/models.rs"), FIXTURE).unwrap();
        Self { dir }
    }

    /// Runs `rango <args>` in the project, against `url` when given.
    pub fn cli(&self, url: Option<&str>, args: &[&str]) -> Output {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_rango"));
        cmd.current_dir(&self.dir)
            .env_remove("DATABASE_URL")
            .env_remove("MIGRATE_DATABASE_URL")
            .stdin(std::process::Stdio::null())
            .args(args);
        if let Some(url) = url {
            cmd.env("DATABASE_URL", url);
        }
        cmd.output().unwrap()
    }

    pub fn snapshot(&self) -> serde_json::Value {
        let raw = std::fs::read(self.dir.join("migrations/snapshot.json")).unwrap();
        serde_json::from_slice(&raw).unwrap()
    }

    /// Concatenated migration SQL, in file order.
    pub fn migrations(&self) -> String {
        let mut files: Vec<_> = std::fs::read_dir(self.dir.join("migrations"))
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "sql"))
            .collect();
        files.sort();
        files
            .iter()
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect()
    }

    pub fn migration_files(&self) -> usize {
        std::fs::read_dir(self.dir.join("migrations"))
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().is_some_and(|x| x == "sql"))
                    .count()
            })
            .unwrap_or(0)
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub mod scenario;

/// Rows of a `rango dump --data-only` as `table -> sorted rows`, so two dumps compare
/// regardless of row order.
pub fn dump_rows(
    p: &Project,
    url: &str,
    only: &[String],
) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut args = vec!["dump", "--data-only", "-o", "d.json"];
    for t in only {
        args.extend(["-t", t]);
    }
    assert_ok(&p.cli(Some(url), &args));
    let raw = std::fs::read(p.dir.join("d.json")).unwrap();
    std::fs::remove_file(p.dir.join("d.json")).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let mut map = std::collections::BTreeMap::new();
    for t in v["tables"].as_array().unwrap() {
        let mut rows: Vec<String> = t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.to_string())
            .collect();
        rows.sort();
        map.insert(t["name"].as_str().unwrap().to_string(), rows);
    }
    map
}

/// The whole project flow, minus the backend specifics. `drop_all(tables)` removes the
/// given tables (and `_rango_migrations`); `exercise` runs the compiled-model queries.
pub async fn flow<D, DF, E, EF>(p: &Project, url: &str, drop_all: D, exercise: E)
where
    D: Fn(Vec<String>) -> DF,
    DF: std::future::Future<Output = ()>,
    E: FnOnce() -> EF,
    EF: std::future::Future<Output = ()>,
{
    assert_ok(&p.cli(None, &["makemigrations", "src", "--yes"]));
    let mut tables: Vec<String> = p.snapshot()["tables"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert!(tables.iter().all(|t| t.starts_with("rpx_")), "{tables:?}");
    tables.push("_rango_migrations".into());

    drop_all(tables.clone()).await;
    assert_ok(&p.cli(Some(url), &["migrate"]));

    // Nothing changed since: no new migration.
    let before = p.migration_files();
    assert_ok(&p.cli(None, &["makemigrations", "src", "--yes"]));
    assert_eq!(
        p.migration_files(),
        before,
        "second makemigrations must be a no-op"
    );

    exercise().await;

    // dump -> wipe -> restore: same rows.
    let snap = p.snapshot();
    let dumped: Vec<String> = snap["tables"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let original = dump_rows(p, url, &dumped);
    assert!(original.values().any(|r| !r.is_empty()));
    let mut args = vec!["dump", "-o", "full.json"];
    for t in &dumped {
        args.extend(["-t", t]);
    }
    assert_ok(&p.cli(Some(url), &args));
    drop_all(tables.clone()).await;
    assert_ok(&p.cli(Some(url), &["restore", "full.json"]));
    assert_eq!(dump_rows(p, url, &dumped), original, "rows after restore");
    // Leave the database as found: other suites list every table they see.
    drop_all(tables).await;
}

pub fn assert_ok(o: &Output) {
    assert!(
        o.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}
