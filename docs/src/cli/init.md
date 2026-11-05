# rango init

Initializes a new Rango project by generating `rango.toml` and the `migrations/` directory.

## Usage

```bash
rango init                    # SQLite backend (default)
rango init --backend postgres # PostgreSQL backend
rango init --backend sqlite   # explicit SQLite
```

## Generated files

**`rango.toml`:**
```toml
[database]
backend = "sqlite"
# url = "sqlite://db.sqlite3"
# Tip: set DATABASE_URL env var instead of storing credentials here.

[models]
src = "src"

[migrations]
dir = "migrations"
```

**`migrations/`:** Empty directory, ready for generated migration files.

## Next steps

```
1. Define your models with #[derive(Model)]
2. Run `rango makemigrations` to generate SQL migrations
3. Run `rango migrate` to apply them
```

## Notes

- `rango init` fails if `rango.toml` already exists
- The `url` field is commented out by default — use `DATABASE_URL` env var for credentials
- The backend field (`sqlite` or `postgres`) determines the SQL dialect for `makemigrations`
