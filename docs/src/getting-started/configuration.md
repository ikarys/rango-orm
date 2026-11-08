# Configuration

Rango is configured via `rango.toml` at the root of your project.

## rango.toml

```toml
[database]
backend = "sqlite"   # "sqlite", "postgres", "mysql" or "mariadb"
# url is optional here — prefer DATABASE_URL env var for credentials

[models]
src = "src"          # directory to scan for #[derive(Model)] (a path given to `rango makemigrations` overrides it)

[migrations]
dir = "migrations"   # directory for generated SQL files
```

## Database URL

The database URL is read from (in priority order):

1. `--database-url` CLI argument
2. `DATABASE_URL` environment variable
3. `url` field in `rango.toml` (local dev only — do not commit credentials)

`rango migrate` also reads `MIGRATE_DATABASE_URL` before all of these: see [migrate](../migrations/migrate.md).

```bash
# Recommended for production
export DATABASE_URL=postgres://user:password@host:5432/mydb

# For local dev
DATABASE_URL=sqlite://db.sqlite3 rango migrate
```

## Backends

| `backend` | URL format | Notes |
|---|---|---|
| `sqlite` | `sqlite://path/to/db.sqlite3` | In-memory: `sqlite::memory:` |
| `postgres` | `postgres://user:pass@host:5432/db` | Also accepts `postgresql://` |
| `mysql` | `mysql://user:pass@host:3306/db` | MySQL 8.0+ |
| `mariadb` | `mariadb://user:pass@host:3306/db` | MariaDB 10.6+; same driver as `mysql` |

When a command gets a database URL (`-d` or `DATABASE_URL`), the backend is taken from its scheme (`postgres` / `postgresql`, `mysql` / `mariadb`, `sqlite`), whatever `backend` says. `backend` is the fallback for a URL with another form, and decides which SQL `makemigrations` writes.

## Table names

Tables are never prefixed. The table name is the `#[model(table = "...")]` value, or the snake_case of the struct name. `rango makemigrations` and the runtime use the same names.

`[models] prefix` was removed: `makemigrations` fails if it is still set. Use `#[model(table = "...")]` instead.

## Connection pool (Postgres)

`rango::connect_postgres(&config)` reads its settings from `DatabaseConfig`:

```rust
let mut config = DatabaseConfig::from_env()?;
config.statement_timeout_ms = Some(5_000); // cancel queries running longer than 5 s
config.lock_timeout_ms = Some(2_000);      // give up waiting for a lock after 2 s
config.slow_query_threshold_ms = 200;      // WARN log for queries slower than 200 ms
let pool = rango::connect_postgres(&config).await?;
```

| Field | Default | Notes |
|---|---|---|
| `max_connections` / `min_connections` | 20 / 2 | |
| `connect_timeout` | 10 s | Pool acquire timeout → `Error::Timeout` |
| `idle_timeout` | 600 s | |
| `max_lifetime` | 1800 s | Recycles old connections |
| `test_before_acquire` | `false` | Saves a round-trip per acquire; enable behind proxies that drop idle connections |
| `statement_timeout_ms` / `lock_timeout_ms` | server default | Sent at connection startup (PgBouncer: add them to `ignore_startup_parameters`) |
| `slow_query_threshold_ms` | 500 | |

Queries are logged through `tracing` on the `sqlx::query` target: every query at `DEBUG`, slow ones at `WARN`. Only SQL text is logged, never bound values. SQLite uses the same logging with a fixed 500 ms threshold.
