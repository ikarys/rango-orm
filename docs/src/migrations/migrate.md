# rango migrate

Applies pending SQL migration files to the database.

## Basic usage

```bash
DATABASE_URL=postgres://user:pass@localhost/mydb rango migrate
```

Output:
```
🔌 Connecting to database (postgres)...
  ⏭  0001_myapp_user.sql (already applied)
  ▶  Applying 0002_myapp_post.sql...
  ✅ 0002_myapp_post.sql
✅ Applied 1 migration(s).
```

## Options

```bash
# Custom migrations directory
rango migrate --migrations db/migrations

# Custom database URL
rango migrate --database-url postgres://...
```

## Migration tracking

Rango creates a `_rango_migrations` table in your database to track applied migrations:

```sql
-- Postgres
CREATE TABLE _rango_migrations (
    id         SERIAL PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- SQLite
CREATE TABLE _rango_migrations (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL UNIQUE,
    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

## Order of execution

Migrations are applied in alphabetical order (which matches their numbered prefix):

```
0001_myapp_user.sql       ← applied first
0002_myapp_post.sql       ← applied second
0003_myapp_tag_post.sql   ← applied third
```

## Idempotency

Already-applied migrations are skipped. Running `rango migrate` twice is safe.

## DATABASE_URL

The database URL is resolved in this order:

1. `--database-url` CLI flag
2. `DATABASE_URL` environment variable
3. `url` field in `rango.toml`
