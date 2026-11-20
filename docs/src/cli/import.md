# rango import

Imports data from JSON or CSV into the database.

## Usage

```bash
# Import from JSON (format auto-detected from extension)
rango import data.json

# Import into a specific table
rango import users.json --table user

# Import CSV
rango import users.csv --table user

# Replace existing rows on conflict
rango import fixtures.json --replace

# Explicit format
rango import data --format json --table users
```

## Options

| Option | Default | Description |
|---|---|---|
| `--table` | from `__table` field | Target table. Required for a single-table export (`rango export --table x`) and for CSV, which carry no `__table`; without it import stops with an error asking for it. |
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
  { "__table": "user", "id": "...", "email": "alice@example.com" },
  { "__table": "post", "id": "...", "title": "Hello" }
]
```

## CSV format

The first line is the header. Fields follow RFC 4180: quoted fields may contain commas, doubled quotes and line breaks; both LF and CRLF end a record. CSV carries no `__table`, so pass `--table`.

- An unquoted field is typed: empty is `NULL`, then integer, float, `true`/`false`, otherwise text.
- A quoted field is always text (`"42"` stays the string `42`, `""` is an empty string). A quoted JSON object or array is bound to a JSON column as JSON (on Postgres it is parsed first, so `jsonb` gets an object, not a string).

`rango import` reads what `rango export --format csv` writes.

## Fixtures / loaddata

`rango import` is Rango's equivalent of Django's `loaddata`. Use it to seed development databases:

```bash
# Export production data (sanitized)
DATABASE_URL=$PROD_URL rango export --table user --output fixtures/users.json

# Load into dev
DATABASE_URL=sqlite://dev.db rango import fixtures/users.json
```
