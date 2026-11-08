# rango makemigrations

Scans your source code for `#[derive(Model)]` structs, diffs against the last known state, and generates SQL migration files.

## Basic usage

```bash
rango makemigrations
```

Output:
```
🔍 Scanning models in src... (backend: postgres)
Found 3 model(s):
  - user (5 columns)
  - post (6 columns)
  - tag (2 columns)
✅ Migration generated: migrations/0001_post_tag_user.sql
📸 Snapshot updated.
```

The file is named `NNNN_<tables>.sql`: the number is the next free one, followed by the tables the migration touches in alphabetical order.

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

# Confirm every rename and drop without asking
rango makemigrations --yes
```

## Renames

Renaming a field looks like a dropped column plus an added one. When a table has both in the same run, Rango pairs them (columns of the same type first) and asks:

```
Did you rename "user"."name" to "full_name"? [y/N]
```

Answer `y` and the migration contains `ALTER TABLE "user" RENAME COLUMN "name" TO "full_name";`, so the data is kept. Answer anything else and it stays a drop plus an add.

- `--yes` answers `y` to every rename question (and to the destructive-changes confirmation), for scripts. Review the SQL first with `--dry-run`.
- Without a terminal (CI, pipes) Rango cannot ask, so each question counts as "no". If the new column is `NOT NULL` without a default, the command stops with an error that mentions the possible rename and `--yes`. Otherwise a drop that needs confirmation aborts with `Aborted: destructive changes need confirmation (interactive, or --yes)`.
- With `--check`, renames are never confirmed: the run only reports whether migrations are pending.

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
| Field renamed (confirmed at the prompt or with `--yes`) | `ALTER TABLE ... RENAME COLUMN ... TO ...` |
| `#[field(unique)]` added or removed | `ALTER TABLE ... ADD CONSTRAINT ... UNIQUE`, or the backend's drop (SQLite: table rebuild) |
| Index added or removed (`#[field(index)]`, foreign keys) | `CREATE INDEX IF NOT EXISTS ...` / `DROP INDEX ...` |
| Foreign key target or `on_delete` changed | `DROP CONSTRAINT` + `ADD CONSTRAINT ... REFERENCES ...` (SQLite: table rebuild) |
| `CheckConstraint` / `UniqueConstraint` added or removed | `ALTER TABLE ... ADD/DROP CONSTRAINT ...` (SQLite: table rebuild) |
| Partial unique constraint (`.condition(..)`) added or removed | `CREATE UNIQUE INDEX ... WHERE ...` / `DROP INDEX` (Postgres and SQLite; skipped with a comment on MySQL / MariaDB) |

## SQLite limitations

SQLite cannot change a column's type, nullability, FK or uniqueness, nor add or drop a
constraint, in place. For those changes Rango rebuilds the table instead:

```sql
-- SQLite cannot alter columns or constraints in place: rebuild "user".
CREATE TABLE "_rango_new_user" (...);
INSERT INTO "_rango_new_user" ("id", "name", ...) SELECT "id", "name", ... FROM "user";
DROP TABLE "user";
ALTER TABLE "_rango_new_user" RENAME TO "user";
CREATE INDEX ...;
```

`rango migrate` runs it with foreign keys off, so dropping the old table never cascades
into child tables, then runs `PRAGMA foreign_key_check` before committing. A migration that
leaves a dangling FK, or copies a `NULL` into a new `NOT NULL` column, is rolled back.

## CI guard

```yaml
# .github/workflows/ci.yml
- name: Check no pending migrations
  run: rango makemigrations --check
```

This fails if any model changes are not yet migrated — useful to catch forgotten `makemigrations` in PRs.
