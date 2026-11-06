# Changelog

## 0.1.0 (2026-11-06)

First stable-track release: the full v0.1.0 to-do list is in.

- Partial selects (only / defer): `#[derive(Partial)]` + `#[partial(model = User)]` on a struct holding a subset of the model's columns, then `.select_as::<UserLite>().all()` / `.one()`. The `SELECT` lists only those columns; names and types are checked at compile time.
- Implicit primary key: `#[rango::model]` injects `pub id: FieldUuid` when the struct declares no primary key (`pk = "bigint"` / `"int"` for database-generated ids), then derives `Model`.
- CLI: `rango dump` (`--schema-only`, `--data-only`, `--table`, `--output`), `rango restore <file>` and `rango backup`. Backend-independent Rango JSON format (schema snapshot + rows), gzip for backups, files written `0600`. Restore treats the file as untrusted (names checked against the dump schema and quoted, values bound, decompressed size capped) and runs in one transaction.
- Keyset pagination: `.after(col, value)` / `.before(col, value)`, constant cost per page unlike `OFFSET`.
- Tracing: every database operation emits a `db.query` span with OpenTelemetry fields (`db.system`, `db.operation`, `db.sql.table`, `db.rows`).
- Perf (Postgres): `bulk_create` is one `INSERT … SELECT FROM UNNEST(…)` statement, one array per column, with no parameter-limit chunking (multi-row `VALUES` kept for JSON columns).
- Fix: `one()` ignored `order_by` and `offset`; it now runs the same query as `all()` capped at one row.
- Fix: `#[derive(Model)]` recognises field types written with a path (`rango::FieldUuid`, `rango::ForeignKey<User>`).
- Fix: `rango export` on Postgres wrote `null` for uuid, timestamp and other non-JSON-native columns; it now serialises every column.
- Change: `rango import` on SQLite fails on rows violating a constraint instead of skipping them silently.
- Fix: `rango import --replace` on Postgres generated invalid SQL (`ON CONFLICT DO UPDATE` without a conflict target); it now targets the primary key.
- Fix (MySQL / MariaDB): `rango import` inserted rows outside the transaction when it also ran DDL, so a failed import left partial rows.
- Fix (breaking for migrations): `makemigrations` no longer prefixes table names with the package name. The `#[derive(Model)]` table name (`table = "..."` or snake_case of the struct) is the only source of truth, so migrations create the tables the runtime queries; M2M pivots use the runtime's `m2m_pivot` names. `--prefix` and `[models] prefix` are removed (`makemigrations` fails if `prefix` is still in rango.toml). With an existing snapshot of prefixed names, the next `makemigrations` sees the old tables as dropped and the new ones as created (table renames are not detected, only column renames): it asks before dropping and would drop the data. Either rename the tables in your database by hand (`ALTER TABLE old RENAME TO new`, pivots too) and edit the snapshot to match, or regenerate the migrations on a fresh database.
- Fix: models with a circular foreign key (A -> B -> A) now migrate on Postgres and MySQL: the forward reference is emitted as `ALTER TABLE ... ADD CONSTRAINT` after all `CREATE TABLE`s.

## 0.1.0-beta.3 (2026-11-06)

First release on crates.io. `rango`, `rango-core` and `rango-cli` were already taken there, so every crate is renamed `rango-orm-*`.

**Breaking (Cargo.toml only):** depend on `rango-orm = "0.1.0-beta.3"` instead of the git `rango` crate. The library is still imported as `rango` (`use rango::prelude::*`), and the CLI binary is still `rango` (`cargo install rango-orm-cli --version 0.1.0-beta.3`). Code that used the inner crates directly renames `rango_core` → `rango_orm_core`, `rango_postgres` → `rango_orm_postgres`, and so on.

- Security (MySQL / MariaDB): TLS support. Remote hosts now default to `ssl-mode=REQUIRED` when the URL sets no mode; before, no TLS was compiled in and connections silently went over cleartext.
- Security: string column names passed to the query builder (filters, `order_by`, subqueries, `values`, aggregates, backend extensions) are checked against the model schema. An unknown name returns `Error::Invalid` before any SQL runs, instead of a database error.
- Minimum supported Rust version is now 1.89 (was declared 1.85, but current dependencies such as `uuid` already need 1.89). CI checks it.

## 0.1.0-beta.2 (2026-11-06)

**Breaking:** on SQLite and MySQL, `QueryBuilder::all()` now returns `Vec<WithRelated<M>>` (derefs to `M`), like Postgres.

- Subqueries on every backend: `where_exists` (correlated `EXISTS`, `NOT EXISTS` via `.not()`) and `in_subquery`. Subquery values stay bound parameters.
- `select_related` is one `LEFT JOIN` over the filtered/paginated rows on every backend.
- `prefetch_related` on every backend, for reverse foreign keys (`Comment::POST_ID`) and many-to-many fields (`Article::TAGS`, one `INNER JOIN` on the pivot). Keys are bound as `= ANY($1)` on Postgres, `IN (...)` in batches of 900 elsewhere.
- Typed relation columns: `select_related` / `prefetch_related` / `where_exists` take `Post::AUTHOR_ID` and infer the related model. `#[derive(Model)]` generates a constant per `ManyToMany` field.
- `M2M` operations (`add`, `remove`, `set`, `all`, `reverse`, `clear`, `exists`) on SQLite and MySQL. `set` runs in one transaction on every backend.
- Perf (Postgres): relation loading uses one pooled connection and decodes straight into `WithRelated`; key dedup is O(n) instead of quadratic.
- Fix: `rango::bulk_create` / `bulk_update` / `bulk_upsert` were missing from the Postgres facade.
- Fix: `ManyToMany` fields no longer trigger `dead_code` warnings.
- Auto-increment primary keys: `FieldAutoInt` / `FieldAutoBigInt` (`GENERATED BY DEFAULT AS IDENTITY` on Postgres, `AUTO_INCREMENT` on MySQL, rowid on SQLite). The database assigns the id on `insert`, `get_or_create` and `bulk_create`. `import` on Postgres moves identity sequences past the imported ids. `makemigrations` refuses a `ManyToMany` between models without UUID keys.
- MySQL 8.0+ / MariaDB 10.6+ backend (`rango-mysql`, feature `mysql`): CRUD, bulk create, transactions, query builder, raw SQL. UUIDs are stored as `BINARY(16)`.
- CLI: `backend = "mysql"` (or `"mariadb"`). `makemigrations` emits backtick-quoted DDL with named unique/index/FK constraints and `MODIFY COLUMN` for type and nullability changes; `migrate` uses an advisory lock; `export`/`import` round-trip UUIDs and JSON, and import runs in one transaction with FK checks off.
- Fix: `import` on Postgres and SQLite failed when a child table came before its parent in the file. Postgres now inserts tables in foreign key order, SQLite defers FK checks to commit, and both run the whole import in one transaction.
- Fix: `makemigrations` ignored `#[model(constraints = ...)]` and `ordering`, and failed on `ManyToMany` fields (the scanner printed types with a repetition instead of an interpolation).

## 0.1.0-beta.1 (2026-11-05)

First beta. The API may still change between betas: pin the tag.

- `#[derive(Model)]` with typed field types, constraints, ordering and FKs with real `ON DELETE`.
- PostgreSQL and SQLite backends: CRUD, bulk create/update/upsert, transactions (`atomic`, `atomic_retry` on Postgres), raw SQL.
- Typed query builder: `Column<M, T>` constants, so comparing a column to a value of the wrong type does not compile.
- Typed errors (`rango::Error`), TLS by default for remote Postgres, argon2id `FieldPassword`.
- `rango` CLI: `init`, `makemigrations`, `migrate`, `export`, `import`.
