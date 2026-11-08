//! Keeps the Quick Start (docs/src/getting-started/quick-start.md) compiling and running.
// The prelude exports one backend's ops (postgres first): SQLite ops are imported by path.
#![cfg(feature = "sqlite")]

use rango::prelude::*;
use rango::sqlite::{RangoFilterExt, insert, raw_execute};

#[derive(Model)]
pub struct User {
    pub id: FieldUuid,
    #[field(unique)]
    pub email: FieldEmail,
    pub name: FieldVarchar<1, 100>,
    pub active: FieldBool,
    pub created_at: FieldDateTime,
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn quick_start_runs() -> TestResult {
    let path = std::env::temp_dir().join(format!("rango-qs-{}.db", uuid::Uuid::new_v4()));
    let pool = connect_sqlite(&format!("sqlite://{}", path.display())).await?;
    raw_execute(
        &pool,
        "CREATE TABLE \"user\" (id TEXT PRIMARY KEY, email TEXT NOT NULL UNIQUE, name TEXT NOT NULL, active BOOLEAN NOT NULL, created_at TEXT NOT NULL)",
        vec![],
    )
    .await?;

    let user = User {
        id: FieldUuid(uuid::Uuid::new_v4()),
        email: FieldEmail::new("alice@example.com")?,
        name: FieldVarchar::new("Alice")?,
        active: FieldBool(true),
        created_at: FieldDateTime(chrono::Utc::now()),
    };
    insert(&pool, user).await?;

    let active_users = User::filter(&pool)
        .eq("active", true)
        .order_by("name")
        .limit(10)
        .all()
        .await?;
    assert_eq!(active_users.len(), 1);
    assert_eq!(active_users[0].name.as_str(), "Alice");
    assert_eq!(active_users[0].email.as_str(), "alice@example.com");
    Ok(())
}
