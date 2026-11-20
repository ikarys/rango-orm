# rango migrate

Applies pending SQL migration files to the database.

## Basic usage

```bash
DATABASE_URL=postgres://user:pass@localhost/mydb rango migrate
```

Output:
```
🔌 Connecting to database (postgres)...
  ⏭  0001_user.sql (already applied)
  ▶  Applying 0002_post.sql...
  ✅ 0002_post.sql
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

-- MySQL / MariaDB
CREATE TABLE _rango_migrations (
    id         BIGINT AUTO_INCREMENT PRIMARY KEY,
    name       VARCHAR(255) NOT NULL UNIQUE,
    applied_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
```

## Order of execution

Migrations are applied in alphabetical order (which matches their numbered prefix):

```
0001_user.sql       ← applied first
0002_post.sql       ← applied second
0003_tag_post.sql   ← applied third
```

## Custom SQL in a migration

Some schema objects are not described by models: a GIN index on a JSON column, a full-text index, a stored procedure, a trigger. Write them in a migration file of your own, with the next free number:

```
migrations/
  0003_invoice.sql        ← generated
  0004_invoice_gin.sql    ← yours: CREATE INDEX invoice_meta_gin ON invoice USING GIN (meta);
```

`rango migrate` applies it like any other file, in order. `makemigrations` ignores it (the snapshot only tracks models, so `--check` stays clean) and numbers the next generated file after it.

`rango restore` rebuilds the schema from the dump's snapshot, not from the migration files, so these objects are NOT recreated after a restore. Run the file's SQL again by hand after restoring.

## Idempotency

Already-applied migrations are skipped. Running `rango migrate` twice is safe.

## DATABASE_URL

The database URL is resolved in this order:

1. `--database-url` CLI flag
2. `MIGRATE_DATABASE_URL` environment variable
3. `DATABASE_URL` environment variable

`MIGRATE_DATABASE_URL` lets migrations run as a role that owns the schema (DDL) while the
application's `DATABASE_URL` uses a role limited to reading and writing rows (DML).
