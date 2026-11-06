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

pub fn assert_ok(o: &Output) {
    assert!(
        o.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}
