# SQLite

SQLite is the default Rango backend — ideal for development, testing, and small applications.

## Setup

```toml
# Cargo.toml
[dependencies]
rango = { git = "https://github.com/ikarys/rango-orm", features = ["sqlite"] }
```

```toml
# rango.toml
[database]
backend = "sqlite"
```

```bash
export DATABASE_URL=sqlite://db.sqlite3
# In-memory (for tests):
export DATABASE_URL=sqlite::memory:
```

## Connecting

```rust
use rango::prelude::*;

let pool = connect_sqlite("sqlite://db.sqlite3").await?;
// or
let pool = connect_sqlite("sqlite::memory:").await?;
```

## SQLite-specific features

Import `SqliteQueryExt` to unlock SQLite-only filter methods:

```rust
use rango::prelude::*; // SqliteQueryExt is included

// json_extract — filter by JSON field value
Post::filter(&pool)
    .json_extract_eq("metadata", "$.status", "published")
    .all()
    .await?

// json_extract with LIKE
Post::filter(&pool)
    .json_extract_like("metadata", "$.tags", "%rust%")
    .all()
    .await?

// Check if JSON key exists
Post::filter(&pool)
    .json_extract_exists("metadata", "$.featured_image")
    .all()
    .await?

// FTS5 full-text search (requires a virtual FTS5 table)
Article::filter(&pool)
    .fts5("articles_fts", "rust async")
    .all()
    .await?
```

## Type mapping

SQLite has fewer native types than Postgres. Rango maps transparently:

| Rango type | SQLite column |
|---|---|
| `FieldUuid` | `TEXT` (UUID string) |
| `FieldBool` | `INTEGER` (0/1) |
| `FieldDateTime` | `TEXT` (ISO 8601) |
| `FieldJson` | `TEXT` (serialized JSON) |
| `FieldVarchar` | `TEXT` |

## Limitations vs Postgres

| Feature | Postgres | SQLite |
|---|---|---|
| Native UUID | ✅ | ❌ (stored as TEXT) |
| JSONB operators | ✅ | ❌ (use `json_extract`) |
| `ALTER COLUMN TYPE` | ✅ | ❌ |
| `SET/DROP NOT NULL` | ✅ | ❌ |
| Full-text search | `tsvector` | FTS5 (virtual table) |
| `RETURNING *` | ✅ | ✅ (SQLite ≥ 3.35) |

## Testing with in-memory SQLite

```rust
#[tokio::test]
async fn my_test() {
    let pool = connect_sqlite("sqlite::memory:").await.unwrap();

    // Create tables inline
    sqlx::query("CREATE TABLE users (id TEXT PRIMARY KEY, email TEXT)")
        .execute(&pool)
        .await
        .unwrap();

    // Test your logic...
}
```

No Docker, no external process — fast and isolated.
