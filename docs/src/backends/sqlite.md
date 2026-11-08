# SQLite

SQLite is the default Rango backend — ideal for development, testing, and small applications.

## Setup

```toml
# Cargo.toml
[dependencies]
rango-orm = { version = "0.2", default-features = false, features = ["sqlite"] }
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

## Errors

On SQLite a unique violation, including one on a partial unique index, reports the column names (`user.email`), not the index name, and a foreign key violation carries no constraint name. See [Errors](../operations/errors.md).

## Testing with in-memory SQLite

Create the schema from the migration files `rango makemigrations` generated. Each file is plain SQL, and `raw_execute` runs a string with several statements on SQLite (give it no parameters).

```rust
use rango::prelude::*;

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_user.sql"),
    include_str!("../migrations/0002_post.sql"),
];

async fn test_pool() -> SqlitePool {
    let pool = connect_sqlite("sqlite::memory:").await.unwrap();
    for sql in MIGRATIONS {
        raw_execute(&pool, sql, vec![]).await.unwrap();
    }
    pool
}

#[tokio::test]
async fn my_test() {
    let pool = test_pool().await;
    // Test your logic...
}
```

List the files in order. A new migration means a new line here. No Docker, no external process: fast and isolated.

## Production on PostgreSQL/MySQL, tests on SQLite

`rango.toml` fixes one backend, and `makemigrations` writes SQL for that backend only. A migration folder made for PostgreSQL does not run on SQLite (types, `ALTER`, index syntax differ). The supported setup is a second set of migrations:

1. Keep your main `rango.toml` (`backend = "postgres"`) and `migrations/`.
2. Add a folder, for example `tests-sqlite/`, with its own `rango.toml` that says `backend = "sqlite"`.
3. From that folder, generate the SQLite files from the same models: `cd tests-sqlite && rango makemigrations ../src --output migrations`. Commit them, and re-run after every model change.
4. Load those files in the test helper above (`include_str!("../tests-sqlite/migrations/0001_user.sql")`).

Limits:

- You maintain two migration sets. `rango makemigrations --check` in each folder tells you when one is stale.
- SQLite is not PostgreSQL. Tests do not see native UUID, `JSONB` operators, `DECIMAL` scale, `PgQueryExt` methods, or the locking and error details of the real server.
- Decimal aggregates, partial unique indexes and constraint names behave differently (see above and [Field types](../models/field-types.md)).
- Run a final test pass against the production engine before release.

