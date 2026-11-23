---
date: 2026-11-23
commit: 1beacf0
kind: blind-campaign-a
tool: Rango (rango-orm 0.5.0, path dependency) and rango CLI built from the repo
model: Sonnet 5.5 (claude-sonnet-5-5)
scope: library app (books, authors, tags, members, loans), SQLite then PostgreSQL 17.11 (docker devpg), 0.5.0 `time` types
verdict: 7/10, usable, with doc gaps around dates, JSON and the PostgreSQL port
---

## Brief

Persona: Django developer who knows little SQL. App: books with M2M authors and tags, members with passwords, loans with a partial unique index (one active loan per book), late loans list, paginated member history. Backends: SQLite file, then PostgreSQL (`postgres://dev:dev@127.0.0.1:5432/blind_a`). About 264k rows seeded (149,946 loans).

Blindness: I read only `docs/src/` markdown, `README.md`, rustdoc output built from my app, and compiler/runtime output. I did not open the source, tests, examples, AGENTS.md, CHANGELOG.md, audit/ (other than this brief) or git history, and I did not follow repo paths in compiler errors. Deviations allowed by the brief: path dependency and CLI built from the repo.

Databases: SQLite via sqlx (version not queried), PostgreSQL 17.11. Friction was logged live in `/tmp/rango-blind-a/friction.md`.

## (a) Friction log

| # | Step | Severity | What happened | Expected | Exact error | Suggested fix |
|---|------|----------|---------------|----------|-------------|---------------|
| 1 | 2, defining models | DOC-WRONG | `docs/models/defining-models.md` type table still lists chrono types (`DateTime<Utc>`, `NaiveDate`, `NaiveTime`) for the date fields. | `time::OffsetDateTime`, `time::Date`, `time::Time`. | none (docs) | Update the table to the 0.5.0 `time` types. |
| 2 | 6, aggregates | DOC-WRONG | `docs/querying/aggregations.md` shows `.min::<time::Date>(..)` and `.max::<time::OffsetDateTime>(..)` with no SQLite caveat (there is one for Decimal). Repro: with feature `sqlite`, `Loan::filter(&pool).max::<OffsetDateTime>(Loan::RETURNED_AT).await?`. Works on PostgreSQL. I used `max::<String>` on SQLite, which returns `"2025-10-02T14:32:19.414022071+00:00"`. | Either it works on SQLite or the page says it does not. | `error[E0277]: the trait bound `for<'r> OffsetDateTime: sqlx_core::decode::Decode<'r, sqlx_sqlite::database::Sqlite>` is not satisfied` (same for `time::Date`, and `Type<Sqlite>`; 12 errors for 2 calls) | Add the caveat and the String workaround, or support the types. |
| 3 | 2/6, dates (release focus) | PAINFUL | No page explains how to build and read the date fields: `FieldDate(Date)`, `FieldDateTime(OffsetDateTime)` found only in rustdoc. Nothing on filters with `time` values, UTC normalisation (read back is always UTC; verified), or the JSON format. A `Date` value against a DateTime column by name is rejected at runtime. | One "Dates and times" page: construction, filters, ordering, group_by_date, JSON, backend notes. | `Invalid("value of type Date does not match column `loaned_at` (DateTime) of table `loan`")` | Add the page with a worked example. |
| 4 | 6, JSON output | PAINFUL | Models do not implement `serde::Serialize`; no doc says so or shows how to produce JSON. I added `#[derive(serde::Serialize)]`. `FieldPassword` has no Serialize, so it needs `#[serde(skip)]` (undocumented). The M2M field serializes as `"authors":[[],null]`. Dates serialize as `"2020-05-01"` and `"2026-11-23T20:55:02.956029864Z"`. | A documented JSON recipe; sane M2M output. | `the trait bound `library::Book: serde::Serialize` is not satisfied` | Document derive plus skip for passwords and M2M; make M2M skip or serialize as ids. |
| 5 | 12/port, PostgreSQL | PAINFUL | `backend` is fixed by `rango.toml`, so the port needed a second project folder `pg/` with its own `rango.toml` and a fresh `makemigrations ../src`. SQLite migration history (6 files with a rebuild) is not reusable. Rust code needed only a cfg `Pool` alias, a different `connect`, and the min/max date types (see 2). | A clear "porting an app" section. | none | Add a porting checklist to the backends docs, including the two-folder pattern. |
| 6 | 3, least privilege | PAINFUL | `DATABASE_URL=<app role> rango migrate` on a database where the role cannot create tables. The message has no hint about `MIGRATE_DATABASE_URL`, and the docs give no GRANT recipe for the app role (schema USAGE, DML, default privileges on tables and sequences for the migration role). I had to drop and recreate the `public` schema with an owner and write the grants myself. | Hint in the error; a copy-paste SQL recipe in the docs. | `Error: Failed to create _rango_migrations` ... `permission denied for schema public at line 2843` | Hint to `MIGRATE_DATABASE_URL` and add the GRANT recipe. |
| 7 | 5, bulk_upsert | PAINFUL | `bulk_upsert` with rows where some have an id and some do not fails with a raw backend error that differs per backend. The all-or-none id rule is documented only for `bulk_create`. | Same rule documented for upsert; a Rango-level error. | SQLite: `database error 1: all VALUES must have the same number of terms`. PostgreSQL: `not null violation` | Document, and validate before sending. |
| 8 | 5, seed | PAINFUL | `bulk_create` returns only a row count. To build loans I re-read authors, books, members with `.unlimited().all()` to get ids. Cumbersome for a Django user (`bulk_create` returns objects there). | A way to get ids back (RETURNING exists on both backends). | none | Add a variant returning ids, or document the re-read pattern. |
| 9 | 7, volume on PostgreSQL | MINOR | Right after the bulk seed (no `ANALYZE`), the member history query (`WHERE member_id=$1 ORDER BY id DESC LIMIT 25`) used `Index Scan Backward using loan_pkey` with `Rows Removed by Filter: 61995` (2.85 ms). After `ANALYZE` it used `loan_member_id_idx` (0.06 ms). No docs mention ANALYZE after a bulk load, and there is no composite non-unique index option (`#[field(index)]` is per column), so `(member_id, id)` cannot be declared. | A note after bulk loads; a composite index attribute. | none | Doc note; consider `Meta` indexes. |
| 10 | 6, select_related | MINOR | `select_related` on `member` loads the argon2 `password` hash column for every row (`__r1_password`); it cannot be excluded. | An option to defer or skip columns. | none | Add `defer`/`only`, or skip FieldPassword in joins. |
| 11 | 11, mistakes | MINOR | `limit(-1)` is accepted on SQLite and returns every row, bypassing the 1000-row default cap. On PostgreSQL it errors. The i64 type of `limit`/`offset` is not stated in the docs (I first passed u64). | Same behaviour on both; reject negatives. | PostgreSQL: `database error 2201W: LIMIT must not be negative`; u64 call: `expected `i64`, found `u64`` | Reject negatives in Rango; document the type. |
| 12 | 9, SQLite race | MINOR | 30 tasks racing `atomic{exists-check; insert}` on SQLite: `ok=1 aborted=28 other=["timed out"]`. One task got a bare timeout with no hint that it is the busy timeout or the pool. On PostgreSQL with `atomic_retry` (SERIALIZABLE): `ok=1 aborted=29 other=[]`, and exactly 1 active loan. | Context in the message. | `timed out` | Say what timed out and the setting. |
| 13 | 11, mistakes | MINOR | Constraint names in errors differ by backend. SQLite: `unique violation (loan.book_id)` for the partial unique index and `foreign key violation` with no name. PostgreSQL: `unique violation (uniq_active_loan)`, `foreign key violation (loan_book_id_fkey)`. Matching on `UniqueViolation { constraint }` is therefore not portable. | Portable constraint info. | `unique violation (loan.book_id)` (SQLite) | Document the difference. |
| 14 | 6, dates on PostgreSQL | MINOR | `timestamptz` keeps microseconds: `7:59:59.999999999` reads back as `7:59:59.999999` and `8:00:00.000000001` as `8:00:00.0` (truncated). SQLite keeps nanoseconds as text. Not documented. | A precision note. | none | Document per-backend precision. |

Extra findings (one line): `order_by("-nme")` gives no "did you mean" when nothing is close; `sum` on a Decimal column returns `f64` (SQL `SUM(fee)::float8`), so exact sums needed reading all rows (505 ms for 82,735 rows); `rango makemigrations` rename needs a TTY or `--yes` (the error does say so); CLI flags are inconsistent (`dump -o file`, `backup --dir`, `restore <input>`), and I guessed wrong once; `ANALYZE` as the `dev` role printed permission warnings for system catalogs (harmless).

## (b) Effort per step

Rust lines are for the code I wrote for that step. SQL counts come from the tracing layer (`sqlx::query` events). Steps marked "not measured" were not counted.

| Step | Lines of code written | Calls or queries made |
|------|-----------------------|-----------------------|
| 1 install, init, config | 20 (Cargo.toml, rango.toml, lib.rs alias) | CLI only |
| 2 models | 83 (models.rs) | n/a |
| 3 makemigrations + migrate | 0 | CLI; 1 migration, applied on both backends |
| 4 evolve schema (add isbn, rename name to full_name, check constraint with SQLite rebuild, add renewals, later waived) | 6 | CLI; 6 SQLite and 2 PostgreSQL migration files |
| 5 seed | about 110 (in app.rs) | 264,347 rows: SQLite 19.4 s (13.6k rows/s incl. 2000 argon2 hashes at 9.4 ms; loans alone 377k rows/s), PostgreSQL 20.4 s (loans 116k rows/s); statement count not measured |
| 6 queries | about 160 (in app.rs, stats.rs 37) | late loans 2 stmts (1 after the first connection's PRAGMA), late count 1, member history keyset 4 stmts for 3 pages plus an empty page, or/not/group 1, where_exists and in_subquery 1 each, prefetch authors and tags 3, group_by_date 1, having 1; PostgreSQL all 1 |
| 7 volume (SQLite) | in app.rs | offset page 500 3.0 ms vs keyset 0.98 ms; page 1400 4.3 ms vs 0.37 ms; late count 8.5 ms; group_by_date month 60 ms; top borrowers 3.5 ms; add column on 150k rows 0.038 s |
| 7 volume (PostgreSQL) | same code | statements 1 each, 0.8 to 12 ms; keyset page 500 0.038 ms (index scan on loan_pkey); late loans use `uniq_active_loan`; add column (`waived`, NOT NULL DEFAULT false, 144,805 rows) 0.024 s |
| 8 transaction | 25 (`borrow_all`) | rollback verified on both backends: loans before and after equal (144,802) |
| 9 concurrency | 50 | SQLite and PostgreSQL results in finding 12 |
| 10 observability | 37 (tracing layer) | slow query log verified on PostgreSQL with `slow_query_threshold_ms = 1`: `slow statement: execution time exceeded alert threshold` |
| 11 mistakes | 63 | about 25 deliberate mistakes on each backend |
| 12 dump, restore, backup | 0 | SQLite: dump 1.26 s (58.3 MB), restore 0.96 s, backup 1.67 s (4.03 MB gz). PostgreSQL: dump 1.2 s (62.2 MB), restore 3.0 s (260,012 rows), backup 1.5 s (4.10 MB gz). SQLite dump restored into empty PostgreSQL in 3.1 s (260,011 rows; same counts and fee sum 418455.50; partial index and sequence OK) |

Total Rust: 763 lines in src.

Not tested: makemigrations evolution (rename, constraint change) on PostgreSQL; the `pg/` folder was generated once from the final models, and only the later `waived` add-column was evolved there.

## (c) What was simple

- Models, hooks (lowercase email, slug), partial unique index and check constraint worked as documented.
- Same app code ran on both backends after a small cfg alias; queries gave the same results.
- `select_related`, `prefetch_related`, `add_many`/`add_pairs`, `group_by_date`, QuerySet `update`/`delete` and keyset pagination issued the expected number of statements. No N+1.
- Error messages for a forgotten migrate (`table `member` does not exist — run `rango makemigrations` and `rango migrate``), wrong value type, unknown column with "did you mean", and check/unique/FK violations were clear.
- Atomic rollback and the PostgreSQL `atomic_retry` race behaved correctly.
- Dump, restore, backup and the cross-backend restore (with a clear note when the migrations folder did not match) worked first time. Time comparisons, ordering and offset normalisation of `OffsetDateTime` were correct on both backends.

## (d) Documentation pages wrong, missing or misleading

- `models/defining-models.md`: wrong type table (chrono).
- `querying/aggregations.md`: min/max on dates misleading on SQLite; Decimal sum returns f64 without a hint.
- Missing: a date/time page (construction, filters, JSON, precision); a JSON/serde page; a PostgreSQL port checklist; the least-privilege GRANT recipe (`getting-started` or migrations pages).
- `operations/bulk.md`: id all-or-none rule absent for `bulk_upsert`.
- Missing: ANALYZE note after bulk load and composite index option; `limit`/`offset` types and negative values.

## (e) Verdict

Score: 7/10. The core is solid: migrations, relations, bulk speed, error messages and statement counts are good, and the port to PostgreSQL was cheap. The 0.5.0 `time` types work, but the docs lag: a wrong type table, a misleading aggregation example that fails to compile on SQLite, and no date, JSON or porting page. A Django user would also miss ids from `bulk_create` and a documented JSON output. I would use Rango for this app on PostgreSQL today, and on SQLite once the min/max date gap is fixed or documented.

## (f) Decisions

Checked against develop at commit 6230602 (2026-11-23).

| # | Decision | Commit or reason |
|---|---|---|
| 1 | fixed | b06601a: the type table lists `time::OffsetDateTime` / `time::Date` / `time::Time`. |
| 2 | fixed | b06601a: the SQLite backend enables sqlx's `time` feature (dropped with `chrono` in 0.5.0); `min`/`max` on `FieldDate` and `FieldDateTime` are tested on SQLite. aggregations.md was right once the feature is back. |
| 3 | open | Partly addressed in b06601a (field-types.md shows how to build and read the types and the serde formats). A full dates-and-times page (filters, UTC, `group_by_date`, backend precision) is still to write. |
| 4 | open | Document `#[derive(serde::Serialize)]` on models with `#[serde(skip)]` for `FieldPassword`, and decide how FK and M2M fields serialize (same as C 5). |
| 5 | open | A "one app, several backends" page: cfg alias, two migration folders, list of differences (same as C 3). |
| 6 | open | Hint at `MIGRATE_DATABASE_URL` in the migrate permission error, and a GRANT recipe per backend in credentials.md (same as B 10, C 14). |
| 7 | open | Validate the all-or-none id rule in `bulk_upsert` before SQL with `Error::Invalid`, and document it (same as C 10). |
| 8 | open | Returning ids from `bulk_create` (RETURNING on PostgreSQL and SQLite, not on MySQL). To decide; meanwhile document the re-read pattern. |
| 9 | open | Doc note on `ANALYZE` after a bulk load. Composite indexes are tracked with B 4. |
| 10 | open | Skip or defer the `FieldPassword` column in `select_related` joins. |
| 11 | open | Reject a negative `limit`/`offset` with `Error::Invalid` on every backend; state the `i64` type in the docs. |
| 12 | open | Say what timed out (pool acquire or SQLite busy) and which setting to change (same as C 6). |
| 13 | open | Document that constraint names in errors differ per backend (same as C 11). |
| 14 | open | Document per-backend time precision (PostgreSQL and MySQL microseconds, SQLite nanoseconds as text). |
