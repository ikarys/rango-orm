# rango dump / restore / backup
One command set, with the same behaviour on PostgreSQL, SQLite, and MySQL/MariaDB. No `pg_dump`/`mysqldump` is needed.

## Format
Dumps use "Rango JSON", a backend-independent JSON object with:
- `format: "rango-dump"`
- `version: 2` (version 1 files are still read)
- `rango_version`
- `backend`
- `created_at`
- `schema`: the latest migration snapshot, or `null`
- `tables`: `[{ "name": ..., "rows": [ {column: value, ...} ] }]`

A dump from one backend can be restored on another.

## dump
```bash
rango dump
```

| Option | Default | Description |
|---|---|---|
| `--schema-only` | — | Schema snapshot, no rows. Conflicts with `--data-only`. |
| `--data-only` | — | Rows, no schema. |
| `-t, --table <name>` | all tables | Table to dump. Repeatable. |
| `-o, --output <file>` | stdout | Output file. Written with `0600` permissions. |
| `-m, --migrations <dir>` | `migrations` / `[migrations] dir` from `rango.toml` | Where the schema snapshot is read. |
| `-d, --database-url` | `DATABASE_URL` | Database URL. |

`--schema-only` prints the number of tables in the schema. The backend comes from the URL scheme, not `rango.toml`.

The `_rango_migrations` bookkeeping table is not dumped as a table (unless named with `--table`); its migration names go in a separate `migrations` field. The schema part needs a snapshot; run `rango makemigrations` first.

## restore
```bash
rango restore <file>
```
Accepts plain JSON or gzip, detected automatically. Rejects files that are not Rango dumps.

File contents are treated as untrusted:
- Table and column names are checked against the dump's schema and quoted.
- Values are bound parameters.
- Decompressed size is capped at 8 GiB against gzip bombs.

Restore creates missing tables from the schema, then inserts rows, in one transaction. When it creates the schema it also recreates `_rango_migrations` with the dumped migration names, so `rango migrate` on the restored database has nothing to replay. A dump made by an older rango has no `migrations` field: restore warns and records nothing, so check `rango migrate` before running it. On any error, such as a constraint violation or a row conflicting with existing data, nothing is restored. On MySQL/MariaDB, DDL auto-commits, so tables created before a failure stay empty.

Restore works whether the target is empty or was just prepared by `rango migrate`: migration names already recorded are skipped, while any other conflicting row still fails the restore. The backend is taken from the `-d` URL scheme (`postgres://`, `mysql://`, `sqlite://`), not from `rango.toml`.

The schema is rebuilt from the dump's snapshot, not by replaying migration files, so migrations that hold hand-written SQL (stored procedures, views, triggers, data changes) are recorded as applied without being run. Restore prints a warning listing the recorded migrations; re-run the custom ones by hand from your `migrations/` directory.

Foreign keys are checked at the end of the transaction, so rows can come in any order, including self-references and FK cycles. On Postgres, a cycle where every FK is `NOT NULL` needs the cycle's constraints to be `DEFERRABLE`; `rango makemigrations` creates them that way, but databases migrated before 0.1.1 keep non-deferrable constraints.

## backup
```bash
rango backup
```
Creates a full dump, gzip-compressed, written to `backups/rango-backup-YYYYMMDD-HHMMSS.json.gz` (UTC) with `0600` permissions.

| Option | Default | Description |
|---|---|---|
| `--dir <dir>` | `backups` | Backup directory. Created if missing. |
| `-d, --database-url` | `DATABASE_URL` | Database URL. |

Restore a backup:
```bash
rango restore backups/rango-backup-20261106-142500.json.gz
```

Decimal values are written as JSON strings (`"0.50"`), so no digit is lost. Binary columns are base64 strings (dumps of version 1 used `\\x` hex and are still read).

Dumps contain all your data. Keep them out of git and treat them as secrets.
