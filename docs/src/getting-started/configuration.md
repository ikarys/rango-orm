# Configuration

Rango is configured via `rango.toml` at the root of your project.

## rango.toml

```toml
[database]
backend = "sqlite"   # "sqlite" or "postgres"
# url is optional here — prefer DATABASE_URL env var for credentials

[models]
src = "src"          # directory to scan for #[derive(Model)]
prefix = "myapp"     # optional: override table prefix (default: Cargo package name)

[migrations]
dir = "migrations"   # directory for generated SQL files
```

## Database URL

The database URL is read from (in priority order):

1. `--database-url` CLI argument
2. `DATABASE_URL` environment variable
3. `url` field in `rango.toml` (local dev only — do not commit credentials)

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

## Table prefix

Tables are prefixed with your Cargo package name by default.

```toml
# Cargo.toml
[package]
name = "my-app"
```

This generates tables like `my_app_user`, `my_app_post` (hyphens → underscores).

Override with:
```toml
# rango.toml
[models]
prefix = "custom"
```
