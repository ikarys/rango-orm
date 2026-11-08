# Changelog

## 0.2.0-beta.1 (2026-11-08)

First beta of the 0.2 cycle. The API may still change between betas: pin the tag.

- Breaking: `atomic`, `transaction` and `atomic_retry` take an async closure: `atomic(&pool, async |tx| { ... }).await?` instead of `|tx| Box::pin(async move { ... })`. The returned future is `Send`. `TxFuture` is removed.
- Breaking: the Postgres `QueryBuilder` fields `explicit_limit`, `select_related_specs` and `prefetch_specs` are no longer public (nothing outside the crate used them; the SQLite and MySQL builders never exposed fields).
- Breaking: `rango::prelude` no longer glob-exports every enabled backend (two backends gave ambiguous names). It holds the backend-agnostic items, the `connect_*` functions, the pool types and the ops of ONE backend: postgres if enabled, else sqlite, else mysql. New `rango::postgres` module next to `rango::sqlite` / `rango::mysql`; each lists that backend's ops, pool and `*QueryExt`. `just check-features` builds every feature combination.
- Breaking cleanup: removed the unused `RangoBackend` trait and the `RangoSqlitePool` / `RangoMySqlPool` newtypes; `RowError`'s field is private (use `RowError::new`); `insert_values` and the constraint-name helpers are `#[doc(hidden)]`. Dropped the unused `sqlx` dependency of `rango-orm`; `rango-orm-postgres` only needs tokio's `time` feature.
- Deps: sqlx 0.9, syn 3, argon2 0.6, toml 1, base64 0.23, criterion 0.8. Minimum supported Rust version is now 1.94 (required by sqlx 0.9). Dynamic SQL built by Rango (identifiers through `quote_ident`, values always bound) is passed to sqlx as `AssertSqlSafe`.
- Docs: subquery decision rules — when to use a correlated `EXISTS` subquery vs a `JOIN` vs a `prefetch` (`docs/src/querying/filter.md`).
- Tests: on-demand end-to-end scenario suite (`just scenarios`), gated behind `integration-scenarios` so it never runs in CI; drives a real blog/CMS app (CRUD, relations, M2M, subquery filter, transactions, bulk) on SQLite in-memory.
- Feat: `ModelHooks` are now called by `insert`, `update`, `delete`, `get_or_create` and `update_or_create` on every backend (bulk ops, query-builder writes and raw SQL never fire them). Add `#[model(hooks)]` to a model to write your own `impl ModelHooks`; otherwise the derive emits an empty one. A hook error is returned as the new `Error::Hook`; `before_*` errors cancel the write, `after_*` errors roll back only inside `atomic()`. Breaking: `ModelHooks` is a supertrait of `Model`, so a hand-written `impl Model` needs `impl ModelHooks for X {}`. `BoxFuture` and `HookResult` are exported from `rango_orm_core`.
- License: dual-licensed under MIT OR Apache-2.0 (was MIT only).
- Feat: SQLite migrations rebuild the table (create, copy, drop, rename) for changes SQLite cannot `ALTER`: column type, nullability, FK, unique, constraints. They emitted a comment before. `rango migrate` runs SQLite migrations with foreign keys off and checks `PRAGMA foreign_key_check` before each commit.
- Feat: `.explain_analyze()` on the query builder: `EXPLAIN ANALYZE` on Postgres and MySQL, `ANALYZE FORMAT=JSON` on MariaDB, `EXPLAIN QUERY PLAN` on SQLite.
- Feat: `rango migrate` reads `MIGRATE_DATABASE_URL` before `DATABASE_URL`, so migrations can use a DDL role and the app a DML-only one.
- Docs: stored procedures and functions through `raw*` (no dedicated API).
- Breaking (pre-1.0): the inner value of `FieldVarchar`, `FieldEmail`, `FieldUrl` and `FieldRange` is private, so `FieldVarchar("".into())` no longer skips validation. Build them with `new(..)` (or `TryFrom`; `Deserialize` now validates too) and read them with `as_str()` / `Deref<Target = str>` / `into_inner()` (`get()` for `FieldRange`). Rows read from the database are not re-validated.
- Fix: `FieldVarchar` and `FieldEmail` length bounds count characters, not bytes.
- Feat: clearer compiler errors. `Model`, `FromRow`, `ModelValues` and `FilterColumn` carry `#[diagnostic::on_unimplemented]` notes ("add `#[derive(Model)]`"), and the blanket filter impls are hidden from error suggestions. The derive reports an unsupported field type on the type itself and names the replacement (`Vec<u8>` → `FieldBytes`, `String` → `FieldText` / `FieldVarchar`, ...). Covered by trybuild UI tests.
- Feat: new `Error::TableMissing { table }` (Postgres 42P01, SQLite "no such table", MySQL 1146) saying to run `rango makemigrations` and `rango migrate`. Unknown column names now suggest the closest column. SQLite `UniqueViolation` (and other constraint errors) carry the failing columns as `constraint`.
- Fix: `rango makemigrations` without a terminal no longer fails with a bare NOT NULL error when a column may have been renamed: it says so and points to `--yes` or a terminal.
- Fix: `ForeignKey<M>` and `ManyToMany<M>` no longer require `M: Clone` / `Default` / `Debug` / `PartialEq`. `ForeignKey<M>` is now `Copy`, `Eq` and `Hash`.
- Feat: transactions everywhere. `Model::filter`, the `M2M` operations, `get_or_create`, `bulk_create`, `bulk_update` and `bulk_upsert` take a pool or a transaction on all three backends: `User::filter(&pool)` still works, inside `atomic()` use `User::filter(&mut **tx)`. Bulk operations run in one transaction (a savepoint inside yours).
- Feat: `bulk_update`, `bulk_upsert` and `update_or_create` on SQLite and MySQL/MariaDB, same signatures as Postgres. `update_or_create` fires update hooks when the row existed, create hooks otherwise. SQLite and MySQL `bulk_update` run one `UPDATE` per row in a transaction.
- Fix: Postgres `update_or_create` updated the row using the primary key carried by the new values; it now updates the row it found. Postgres `bulk_update` validates its column names like the other backends.
- Breaking (pre-1.0): `QueryBuilder<M>` is now `QueryBuilder<'a, M>` and holds a `Conn<'a>` (new, built from `&Pool`, `&mut Connection` or `&mut Transaction`). Postgres `RangoExecutor` and `QueryBuilder::transaction` are removed: use `atomic()`. `.explain_analyze()` takes `&mut self`. `update_or_create` takes `&pool` or `&mut *tx` (was `&mut pool`); M2M operations and `get_or_create` are generic over the executor.

## 0.1.1 (2026-11-06)

**Behavior changes to check before upgrading:**

- Breaking: `FieldDecimal<P, S>` now wraps `rust_decimal::Decimal` (re-exported as `rango::Decimal`) instead of `f64`. Construct it with `FieldDecimal("12.50".parse().unwrap())` or `FieldDecimal::from_decimal(d)`; filters take a `Decimal`, not an `f64`. `SqlValue` gains `Decimal` / `NullDecimal`, and `RangoRow` gains `get_decimal` (custom `RangoRow` impls must add it). SQLite stores decimals in a `NUMERIC` column (was `REAL`); a new migration is not generated for existing columns, which keep working.
- Model `ordering = [...]` is now applied as the default `ORDER BY` of every query builder (it was parsed and ignored). Queries on models that declare `ordering` return rows in that order unless you call `order_by`.
- Dump format version 2: binary columns are base64 strings (version 1 wrote `\x` hex and `null` on some backends). Version 1 files are still read.
- Postgres: FKs that close a cycle are created `DEFERRABLE INITIALLY IMMEDIATE`.

Changes:

- Feat: self-referencing `ManyToMany` (`followers: ManyToMany<User>`). The pivot columns are `from_<table>_id` and `to_<table>_id` when both sides are the same table; other pivots are unchanged. `M2M::add(from, to)` is directional, `reverse` goes the other way.
- Fix: `rango dump` / `restore` round-trip binary columns on every backend.
- Fix: decimals are written as JSON strings in dumps, so no digit or trailing zero is lost (`rango dump` on Postgres and MySQL wrote numbers).
- Fix: Postgres `restore` loads a cycle made only of NOT NULL FKs (`SET CONSTRAINTS ALL DEFERRED`); nullable forward FKs are filled after the inserts.
- Fix: MySQL `AlterColumnIndex` on a TEXT/BLOB column uses the same 191-byte prefix as `CREATE TABLE`; MySQL index creation on such columns no longer fails.
- Fix (SQLite): `ON DELETE RESTRICT` now returns `Error::ForeignKeyViolation` instead of `Error::Database`.
- Fix: DECIMAL/NUMERIC columns are read on MySQL and Postgres; `#[derive(Model)]` generates `FromRow` for `FieldDecimal`, FK `references` and `on_delete` in `schema()`.
- Fix: the derive sets the generated primary key for path-qualified `FieldAutoInt` / `FieldAutoBigInt`.
- Fix: SQLite `DEFAULT CURRENT_TIMESTAMP` instead of `now()`.
- Fix: the CLI scanner accepts path-qualified types and `#[derive(rango::Model)]`.
- Tests: derive/CLI contract test, a real-project end-to-end flow on every backend (migrate, queries, dump, wipe, restore), random model graphs on every backend, and runtime `ON DELETE` effects (cascade, set_null, restrict).

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
