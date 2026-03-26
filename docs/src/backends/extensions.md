# Backend-specific Features

Some database features are not portable across backends. Rango exposes them via backend-specific traits rather than hiding or faking portability.

## Philosophy

If a feature doesn't exist on a backend, it's simply not available — no silent fallbacks, no emulation that changes semantics.

```rust
// This compiles and runs only with the postgres feature
use rango::PgQueryExt;

Post::filter(&pool)
    .json_contains("metadata", json!({"status": "published"}))
    .all()
    .await?
```

If you migrate from Postgres to SQLite, the compiler tells you exactly where backend-specific code is used.

## PgQueryExt (Postgres)

| Method | SQL | Description |
|---|---|---|
| `.fts(col, query)` | `to_tsvector(...) @@ plainto_tsquery(...)` | Full-text search |
| `.fts_lang(col, query, lang)` | Same with custom language | FTS with language config |
| `.json_contains(col, val)` | `col @> val::jsonb` | JSONB containment |
| `.json_has_key(col, key)` | `col ? key` | JSONB key existence |
| `.json_field_eq(col, key, val)` | `col->>'key' = val` | JSONB field equality |
| `.array_contains(col, vals)` | `col @> ARRAY[...]` | Array containment |

## SqliteQueryExt (SQLite)

| Method | SQL | Description |
|---|---|---|
| `.json_extract_eq(col, path, val)` | `json_extract(col, path) = ?` | JSON field equality |
| `.json_extract_like(col, path, pat)` | `json_extract(col, path) LIKE ?` | JSON field LIKE |
| `.json_extract_exists(col, path)` | `json_extract(col, path) IS NOT NULL` | JSON key existence |
| `.fts5(table, query)` | `rowid IN (SELECT rowid FROM fts WHERE fts MATCH ?)` | FTS5 search |

## Adding to the prelude

Both traits are automatically available via `use rango::prelude::*` — no extra imports needed.

## Future extensions

Planned backend-specific features:
- `PgQueryExt`: window functions, `DISTINCT ON`, advisory locks
- `SqliteQueryExt`: `json_each()`, `json_group_array()`
- `MySqlQueryExt` (future MySQL backend)
