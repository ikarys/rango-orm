# rango import

Imports data from JSON or CSV into the database.

## Usage

```bash
# Import from JSON (format auto-detected from extension)
rango import data.json

# Import into a specific table
rango import users.json --table myapp_user

# Import CSV
rango import users.csv --table myapp_user

# Replace existing rows on conflict
rango import fixtures.json --replace

# Explicit format
rango import data --format json --table users
```

## Options

| Option | Default | Description |
|---|---|---|
| `--table` | from `__table` field | Target table |
| `--format` | auto-detected | `json` or `csv` |
| `--replace` | skip | Replace existing rows on conflict |
| `--database-url` | `DATABASE_URL` | Database connection URL |

## Conflict behavior

| Mode | SQL | Use case |
|---|---|---|
| Default (skip) | `INSERT OR IGNORE` / `ON CONFLICT DO NOTHING` | Load fixtures without overwriting |
| `--replace` | `INSERT OR REPLACE` / `ON CONFLICT DO UPDATE` | Sync/upsert data |

## JSON format

```json
[
  { "id": "abc-123", "email": "alice@example.com", "active": true },
  { "id": "def-456", "email": "bob@example.com", "active": false }
]
```

With `__table` (multi-table import without `--table`):
```json
[
  { "__table": "myapp_user", "id": "...", "email": "alice@example.com" },
  { "__table": "myapp_post", "id": "...", "title": "Hello" }
]
```

## Fixtures / loaddata

`rango import` is Rango's equivalent of Django's `loaddata`. Use it to seed development databases:

```bash
# Export production data (sanitized)
DATABASE_URL=$PROD_URL rango export --table myapp_user --output fixtures/users.json

# Load into dev
DATABASE_URL=sqlite://dev.db rango import fixtures/users.json
```
