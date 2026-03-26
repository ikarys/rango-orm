# rango export

Exports data from the database to JSON or CSV.

## Usage

```bash
# Export all tables to stdout (JSON)
rango export

# Export a specific table
rango export --table users

# Export to a file
rango export --table users --output users.json

# CSV format
rango export --table users --format csv --output users.csv

# Custom database URL
rango export --table users --database-url postgres://...
```

## Options

| Option | Default | Description |
|---|---|---|
| `--table` | all tables | Table to export |
| `--format` | `json` | Output format: `json` or `csv` |
| `--output` | stdout | Output file path |
| `--database-url` | `DATABASE_URL` | Database connection URL |

## JSON format

Single table:
```json
[
  { "id": "...", "email": "alice@example.com", "active": true },
  { "id": "...", "email": "bob@example.com", "active": false }
]
```

Multiple tables (no `--table` specified):
```json
[
  { "__table": "myapp_user", "id": "...", "email": "alice@example.com" },
  { "__table": "myapp_post", "id": "...", "title": "Hello" }
]
```

The `__table` field lets `rango import` route rows to the right table automatically.

## CSV format

```csv
id,email,active
abc-123,alice@example.com,true
def-456,bob@example.com,false
```

## Cross-backend export

Export from Postgres, import into SQLite:

```bash
DATABASE_URL=postgres://... rango export --output data.json
DATABASE_URL=sqlite://dev.db rango import data.json
```
