# rango makemigrations

Scans your source code for `#[derive(Model)]` structs, diffs against the last known state, and generates SQL migration files.

## Basic usage

```bash
rango makemigrations
```

Output:
```
🔍 Scanning models in src... (backend: postgres)
📦 Table prefix: myapp
Found 3 model(s):
  - myapp_user (5 columns)
  - myapp_post (6 columns)
  - myapp_tag (2 columns)
✅ Migration generated: migrations/0001_myapp_post_myapp_tag_myapp_user.sql
📸 Snapshot updated.
```

## Options

```bash
# Preview SQL without writing files
rango makemigrations --dry-run

# Exit with code 1 if migrations are pending (for CI)
rango makemigrations --check

# Scan a specific directory
rango makemigrations src/models

# Custom output directory
rango makemigrations --output db/migrations

# Override table prefix
rango makemigrations --prefix myprefix
```

## How it works

1. Rango scans your `src/` directory for Rust files containing `#[derive(Model)]`
2. Extracts the schema (table name, columns, types, constraints) using `syn`
3. Loads the previous schema snapshot from `migrations/snapshot.json`
4. Computes the diff
5. Generates a numbered `.sql` file and updates the snapshot

## Snapshot

The snapshot (`migrations/snapshot.json`) tracks the current known state of your schema. **Commit this file** — it's how Rango knows what has changed.

## What gets detected

| Change | Generated SQL |
|---|---|
| New model | `CREATE TABLE IF NOT EXISTS ...` |
| Removed model | `DROP TABLE IF EXISTS ...` |
| New field | `ALTER TABLE ... ADD COLUMN ...` |
| Removed field | `ALTER TABLE ... DROP COLUMN ...` |
| Field type changed | `ALTER TABLE ... ALTER COLUMN ... TYPE ...` |
| Nullable changed | `ALTER TABLE ... ALTER COLUMN ... SET/DROP NOT NULL` |

## SQLite limitations

SQLite does not support `ALTER COLUMN TYPE` or `SET/DROP NOT NULL`.
Rango emits a comment in the migration file explaining what to do manually:

```sql
-- SQLite does not support ALTER COLUMN TYPE on "myapp_user"."score" → recreate the table manually.
```

## CI guard

```yaml
# .github/workflows/ci.yml
- name: Check no pending migrations
  run: rango makemigrations --check
```

This fails if any model changes are not yet migrated — useful to catch forgotten `makemigrations` in PRs.
