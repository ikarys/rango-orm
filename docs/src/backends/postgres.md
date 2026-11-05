# PostgreSQL

PostgreSQL is the primary Rango backend with full feature support.

## Setup

```toml
# Cargo.toml
[dependencies]
rango = { git = "https://github.com/ikarys/rango-orm", features = ["postgres"] }
```

```toml
# rango.toml
[database]
backend = "postgres"
```

```bash
export DATABASE_URL=postgres://user:password@localhost:5432/mydb
```

## Connecting

```rust
use rango::prelude::*;

let config = DatabaseConfig::from_env()?;
let pool = connect_postgres(&config).await?;
```

## Postgres-specific features

Import `PgQueryExt` to unlock Postgres-only filter methods:

```rust
use rango::prelude::*; // PgQueryExt is included

// Full-text search
let articles = Article::filter(&pool)
    .fts("content", "rust async orm")
    .all()
    .await?;

// JSONB containment
let posts = Post::filter(&pool)
    .json_contains("metadata", serde_json::json!({"status": "published"}))
    .all()
    .await?;

// JSONB key existence
let posts = Post::filter(&pool)
    .json_has_key("metadata", "featured_image")
    .all()
    .await?;

// JSONB field equality
let posts = Post::filter(&pool)
    .json_field_eq("metadata", "status", "draft")
    .all()
    .await?;

// Array containment
let users = User::filter(&pool)
    .array_contains("roles", vec!["admin".into(), "moderator".into()])
    .all()
    .await?;
```

## Standalone helpers

```rust
use rango::Pg;

// Full-text search standalone
let results = Pg::fts::<Article>(&pool, "content", "query").await?;

// Get a JSONB field value
let status = Pg::json_get::<Post>(&pool, &post_id, "metadata", "status").await?;
```

## Supported types

All Rango field types are fully supported on PostgreSQL with their native SQL types.
See [Field Types](../models/field-types.md) for the complete mapping.
