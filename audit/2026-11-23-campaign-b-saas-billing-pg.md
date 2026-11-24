---
date: 2026-11-23
commit: 1beacf0
kind: blind-B-saas-billing-pg
tool: Rango (rango-orm 0.5, path dependency) + rango CLI built from the repo
model: Sonnet 5.5 (claude-sonnet-5-5)
scope: multi-tenant SaaS billing data layer, PostgreSQL 17.11, persona = backend engineer who knows SQL and distrusts ORMs
verdict: 7/10, usable for this app; money exactness and a few missing SQL primitives hurt
---

## Brief

Campaign B of the 0.5.0 audit. I built a library crate (`models`, `ops`, `queries`, `seed`, `scenarios`) and a binary `run` with one subcommand per step in `/tmp/rango-blind-b`. Database: PostgreSQL 17.11 in `devpg`, with a migration role `app_migrate` (DDL) and a DML-only role `app`, created as the docs describe. Everything below was run for real; no mocks. Volumes: 20 tenants, 500 products, 20k customers (JSON metadata), 30k invoices, 150k invoice lines, about 18k payments (259k rows).

Blindness: I read only the mdBook sources under `docs/src/`, the README, the compiler/runtime output and the brief. I never opened source, tests, examples, audit files or git history. Compiler errors that print repo paths were not followed. The friction log was written to `FRICTION.md` as events happened, and this report is built from it. Rango was a path dependency (allowed deviation) and the CLI was built with `cargo install --path ... --root .tools`.

## (a) Friction log

| # | Step | Severity | What happened | Expected | Exact error | Suggested fix |
|---|---|---|---|---|---|---|
| 1 | 2/11 decimals | PAINFUL | `FieldDecimal<14,2>` is plain `NUMERIC` on Postgres. Inserting `123456789012345.5` into `invoice.total` succeeds, stored as `123456789012345.50` (15 integer digits in a 14,2 column). 1.005 style over-scale is rounded or accepted without an error. The `OutOfRange` error documented for "too many digits" can therefore never fire on PG | `NUMERIC(14,2)` or an `Err` | none: `[decimal too many digits] OK (no error!)` | Emit `NUMERIC(p,s)` on PG, or validate precision in Rust, or document loudly that precision is not enforced |
| 2 | 6 aggregates | PAINFUL | `sum`, `avg`, `annotate_sum` return `f64` (`sum(total)` = 10040437.08 as double). A money dashboard cannot use them; I had to drop to `raw_values`, where `SUM(numeric)` does decode as `Decimal` (exact) | `annotate_sum` returning Decimal for decimal columns | n/a | Return `SqlValue::Decimal` for decimal columns, or document the `raw_values` recipe on the aggregations page |
| 3 | 6/8 | PAINFUL | No `SELECT ... FOR UPDATE`, no `col = col + 1` in `.set()`. Invoice number allocation needs `raw_scalar("UPDATE tenant SET next_invoice_no = next_invoice_no + 1 WHERE id = $1 RETURNING next_invoice_no - 1", ...)`. This works and is atomic, but the raw-sql page never shows UPDATE ... RETURNING via `raw_scalar` | an expression setter or lock clause, or a documented recipe | n/a | Add an "atomic counter / allocate a number" recipe to transactions.md |
| 4 | 2 | PAINFUL | No composite index declaration (only `#[field(index)]` on one column; constraints are check/unique). `(tenant_id, issued_at)`, `(tenant_id, id)`, GIN on JSON and `to_tsvector` all needed a hand-written migration. After `rango restore`, these indexes are silently gone (documented only as "hand-written SQL is not replayed", the note mentions "functions, triggers", not indexes), which is a performance cliff on restore | `Index::on(&[..])` in `constraints` | n/a | Add composite/expression indexes to the model attribute, and say "indexes" in the restore note |
| 5 | 9 | PAINFUL | `tokio::spawn` of a function whose `atomic(pool, async \|tx\| {...})` body contains `raw_scalar`, `insert`, `Product::filter(&mut **tx)...all()` and `bulk_create` does not compile. The 5 errors point at the spawn site, not at the call that causes it; a smaller closure (`pay_cas`) spawns fine. Workaround: `LocalSet` + `spawn_local`. Not documented | `spawn(atomic(..))` works, or the page states the limit | `implementation of `Send` is not general enough` / `implementation of `AsyncFnOnce` is not general enough` / `implementation of `sqlx_core::acquire::Acquire` is not general enough` | Document it in transactions.md with the workaround, and find which builder call makes the future non-Send |
| 6 | 6 | DOC-WRONG | select-as.md says select_as accepts the same filters, ordering, limit/offset as `.all()`, but `.select_as::<P>().limit(50)` does not compile; limit/offset must come before `select_as` | doc matches behaviour | `error[E0599]: no method named `limit` found for struct `SelectAs`` | State the call order, or add `limit`/`offset` to `SelectAs` |
| 7 | 6 | MINOR | `.in_values(Product::ID, Vec<Uuid>)` is rejected; it needs `Vec<SqlValue>`. The typed-column promise does not extend to `in_values`, and the docs show `.in_values("id", ids)` without the type | `Vec<T>` of the field type | `error[E0308]: mismatched types ... expected `Vec<SqlValue>`, found `Vec<Uuid>`` | Accept `IntoIterator<Item = T>` for typed columns |
| 8 | 6 (0.5 focus) | MINOR | `group_by_date` returns `SqlValue::Text("2025-01-01")`, not a `time::Date`; there is no typed way to get a date back from `aggregate()`. JSON formats differ: serde output of `OffsetDateTime` is `2026-03-23T11:53:51Z` while `rango export` writes `2026-11-23T20:56:47.848984+00:00`, `Decimal` is a string in both. The docs do not state the JSON format of time fields, nor that `FieldDateTime` values are read back as UTC | one documented format | n/a | Add a "time fields: filters, ordering, JSON, UTC" page; mention `time` needs feature `macros` for `date!`/`datetime!` (docs say `time = "0.3"`) |
| 9 | 3 | MINOR | `MIGRATE_DATABASE_URL=` (set but empty) is not skipped | skip empty, or name the variable | `Error: Failed to connect to Postgres` / `Caused by: invalid: relative URL without a base` | Treat empty as unset or name the variable |
| 10 | 3/12 | MINOR | The DML-only role running `migrate` (or `restore`, which needs to create tables) gets an error that names neither the role nor `MIGRATE_DATABASE_URL`; `restore` and `import` ignore `MIGRATE_DATABASE_URL` and the docs do not say so. "line 2843" is noise | hint at `MIGRATE_DATABASE_URL` | `Error: Failed to create _rango_migrations` / `Caused by: 0: error returned from database: permission denied for schema public at line 2843` | Add the hint, and say in dump.md which URL restore uses |
| 11 | 12 | MINOR | `rango import` of rows that all already exist prints `Imported 20 row(s)` although 0 rows were inserted (default skip mode) | count of rows actually inserted | `✅ Imported 20 row(s) from out/tenant.json` | Report inserted vs skipped |
| 12 | 2/4 | MINOR | Default FK `ON DELETE CASCADE` goes tenant -> customer -> invoice -> payment; dangerous for financial data (documented, but not flagged as a hazard). Also `#[field(default = "'EUR'")]` needs SQL-quoted strings, only `default = "0"` is shown | `restrict` default or a warning, a string example | n/a | Add the example and a note in relations.md |
| 13 | 7 | MINOR | Migration docs say nothing about table rewrites and locks. `ADD COLUMN ... DEFAULT` was 25 ms on the 150k-line table (metadata-only on PG17), but a volatile default (`gen_random_uuid()`) rewrote it in 357 ms under an ACCESS EXCLUSIVE lock; nothing about `CREATE INDEX CONCURRENTLY` | a short "large tables" note | n/a | Add a paragraph to migrate.md |
| 14 | 11 | MINOR | `Error::Invalid` for a wrong-type value prints Rust Debug of the column type; `Error::Timeout` is a bare variant for `statement_timeout` (can't tell pool timeout from statement timeout) | readable text | `invalid: value of type Text does not match column `total` (Decimal { precision: 14, scale: 2 }) of table `invoice`` | Format the type as `NUMERIC(14,2)`; distinguish timeout kinds |
| 15 | 1 | MINOR | No guidance for a lib + bin layout (quick start uses a single bin); compiler errors print repo paths (not followed); `get_or_create` and `bulk_upsert` overwrite the whole row on conflict (documented; my `next_invoice_no` was reset) | n/a | n/a | n/a |

Extra, one line: `makemigrations --dry-run` assumes renames; the rename prompt, `--yes` and `--dry-run` work; `json_contains` explain on 20k rows shows a Seq Scan because the default 1000-row limit and 33% selectivity apply (GIN index is used by fts).

## (b) Effort per step

| Step | Lines of code written | Calls or queries made |
|---|---|---|
| 1-2 models (`models.rs`) | 104 | n/a |
| 3-4 migrations (0005 hand SQL, 0006) | about 40 SQL | 6 migrations applied, `migrate` 25 ms for the add-column one |
| 5 seed (`seed.rs`) | 169 | 71 statements, 259k rows, 2.44 s; 150k lines in 1.17 s (128k rows/s) |
| 6-7 queries (`queries.rs` + `main.rs`) | 115 + 137 | filtered: 1 stmt, 1-5 ms; prefetch and M2M prefetch: 2 stmts; json_search x4: 4 stmts; revenue per tenant per month: 440 groups, 1 stmt, 12.9 ms; keyset vs offset page 500: 1.1 ms vs 2.7 ms, identical rows |
| 8 transaction (`ops.rs`) | 111 | `issue_invoice` 6 stmts; failing issue rolled back (counter 1502 before and after, no row) |
| 9 concurrency | in `scenarios.rs` (158 total) | 8 concurrent issues gave 8 distinct numbers; CAS: 1 ok, 7 rejected, 1 payment; SERIALIZABLE `atomic_retry`: 6 workers, 11 attempts (real 40001 retries), 1 payment |
| 10 observability (`obs.rs`) | 50 | custom Layer on `sqlx::query`; slow log at 50 ms: pg_sleep(0.2) gave 1 WARN; `statement_timeout_ms` 1 s cancelled `pg_sleep(3)` with `Timeout` |
| 11 mistakes | in `scenarios.rs` | see below |
| 12 dump, restore, backup | 0 (CLI) | `dump` 77.4 MB JSON in 1.4 s; `backup` 8.2 MB gz in 1.9 s; `restore` of 258,628 rows into an empty DB in 3.5 s; sums, counts and JSON containment identical; hand-written indexes absent after restore |

Deliberate mistakes (all errors readable): unique `unique violation (tenant_slug_key)` and `(uniq_invoice_number_per_tenant)`; second draft `(uniq_one_draft_per_customer)`; FK `foreign key violation (invoice_customer_id_fkey)`; `check violation (chk_invoice_total_nonneg)`; unknown column `unknown column `totl` on table `invoice`, did you mean `total`?`; missing table `table `nope` does not exist — run `rango makemigrations` and `rango migrate``; `update` of a missing row `row not found`; `get` of a missing row gives `None`. Typed columns turn a wrong-type filter into a compile error. I did not trigger `TableMissing` through a model on an unmigrated DB, only through raw SQL.

## (c) What was simple

- Models, derive, typed columns; `makemigrations` / `migrate`, rename prompt and `--check`.
- Partial unique index and `UniqueConstraint` per tenant; constraint names show up in errors.
- `bulk_create` with UNNEST (128k rows/s), `M2M::add_pairs`.
- Statement counts are exactly as documented (select_related 1, prefetch 2); no N+1; counting `sqlx::query` events works.
- `explain` / `explain_analyze`, keyset `.before()`, `get_or_create`, `update_or_create`, `bulk_update`, `bulk_upsert`, M2M ops, `atomic_retry` under a real conflict.
- Dump, restore, backup and export/import need no external tool and are fast.
- `time` fields work in filters, ordering, min/max and group by month.

## (d) Documentation pages wrong, missing or misleading

- `querying/select-as.md`: wrong about limit/offset after `select_as`.
- `models/field-types.md`: Decimal precision not enforced on PG, and `operations/errors.md` `OutOfRange` implies it is.
- `querying/aggregations.md`: f64 sums for decimals; no exact-money recipe.
- `operations/transactions.md`: missing atomic counter recipe and the `Send` limitation of `atomic` futures.
- `models/constraints.md`: no composite or expression index; `cli/dump.md`: restore loses indexes and uses `DATABASE_URL`, not `MIGRATE_DATABASE_URL`.
- `getting-started/installation.md`: `time` needs `macros`; no page on time-field JSON, UTC and aggregation key types.
- `migrations/migrate.md`: nothing on rewrites, locks, concurrent index builds.

## (e) Verdict

Score 7/10. I would use Rango for this app for CRUD, bulk loading, pagination, relations, constraints and the operational tools; the SQL it generates is predictable and the counts match the docs. I would not trust it for money without changes: unbounded `NUMERIC`, f64 aggregates and no precision check are what a billing engineer notices first. I would also keep raw SQL for counters and locking, and hand-write migrations for indexes.

Counts: BLOCKER 0, PAINFUL 5, DOC-WRONG 1, MINOR 9.

## (f) Decisions

Checked against develop at commit 6230602 (2026-11-23).

| # | Decision | Commit or reason |
|---|---|---|
| 1 | fixed in 9b78444 | Migrations create `NUMERIC(P, S)` on PostgreSQL, so an out-of-range value is `Error::OutOfRange`. Existing columns need a hand-written `ALTER TABLE`, documented in `field-types.md`. |
| 2 | open | Return `Decimal` from `sum`/`avg`/`annotate_sum` on decimal columns, or document the `raw_values` recipe. |
| 3 | open | Document the atomic counter recipe (`raw_scalar` with `UPDATE ... RETURNING`) in transactions.md; `FOR UPDATE` and column expressions in `.set()` to decide. |
| 4 | open | Composite and expression indexes in the model attribute; restore note to say indexes are not replayed. |
| 5 | open | Find which builder call makes the `atomic` future non-`Send` (rango-tests/tests/spawn_atomic.rs covers a smaller body), fix or document the `LocalSet` workaround. |
| 6 | fixed | b06601a: select-as.md says `limit`/`offset` go before `select_as`. |
| 7 | open | Accept a `Vec` of the field type in `in_values` for typed columns. |
| 8 | open | field-types.md states the serde formats (b06601a); `group_by_date` returning `Text` and the `time` `macros` feature remain to document or change. |
| 9 | open | Treat an empty `MIGRATE_DATABASE_URL` as unset. |
| 10 | open | Same as A 6; also say in dump.md which URL `restore` and `import` use. |
| 11 | fixed in f698ba6 | The summary reports new and already-present rows (same as C 2). |
| 12 | open | Add a quoted string `default` example and a hazard note on cascading deletes for financial data in relations.md. |
| 13 | open | A short "large tables" paragraph in migrate.md (rewrites, locks, `CREATE INDEX CONCURRENTLY`). |
| 14 | open | Print column types as SQL in `Error::Invalid`; tell pool timeout from statement timeout. |
| 15 | no change | Notes without a requested change; the overwrite on conflict is documented. |
