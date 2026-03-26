# Quick Start

This guide walks you through a complete Rango setup in under 5 minutes.

## 1. Initialize

```bash
cargo new my-app && cd my-app
rango init --backend sqlite
```

Add Rango to `Cargo.toml`:
```toml
[dependencies]
rango = { git = "https://github.com/ikarys/rango-orm", features = ["sqlite"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

## 2. Define a model

Create `src/models.rs`:

```rust
use rango::prelude::*;

#[derive(Model)]
pub struct User {
    pub id: FieldUuid,
    #[field(unique)]
    pub email: FieldEmail,
    pub name: FieldVarchar<1, 100>,
    pub active: FieldBool,
    pub created_at: FieldDateTime,
}

#[derive(Model)]
pub struct Post {
    pub id: FieldUuid,
    pub author_id: ForeignKey<User>,
    pub title: FieldVarchar<1, 255>,
    pub body: FieldText,
    pub published: FieldBool,
}
```

## 3. Generate migrations

```bash
rango makemigrations
```

Output:
```
🔍 Scanning models in src... (backend: sqlite)
📦 Table prefix: my_app
Found 2 model(s):
  - my_app_user (5 columns)
  - my_app_post (5 columns)
✅ Migration generated: migrations/0001_my_app_post_my_app_user.sql
```

## 4. Apply migrations

```bash
DATABASE_URL=sqlite://db.sqlite3 rango migrate
```

## 5. Use the ORM

```rust
use rango::prelude::*;

mod models;
use models::User;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = connect_sqlite("sqlite://db.sqlite3").await?;

    // Insert
    let user = User {
        id: FieldUuid(uuid::Uuid::new_v4()),
        email: FieldEmail("alice@example.com".into()),
        name: FieldVarchar("Alice".into()),
        active: FieldBool(true),
        created_at: FieldDateTime(chrono::Utc::now()),
    };
    let user = insert(&pool, user).await?;

    // Query
    let active_users = User::filter(&pool)
        .eq("active", true)
        .order_by("name")
        .limit(10)
        .all()
        .await?;

    for u in &active_users {
        println!("{} — {}", u.name.0, u.email.0);
    }

    Ok(())
}
```
