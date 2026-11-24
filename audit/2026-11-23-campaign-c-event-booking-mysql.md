---
date: 2026-11-23
commit: 1beacf0
kind: blind-C-event-booking-mysql
tool: Rango 0.5.0 (rango-orm, path dependency, default-features = false) + rango CLI built from the repo
model: Sonnet 5.5 (claude-sonnet-5-5)
scope: MySQL 8.4 (3307), MariaDB 11.4 (3308), SQLite file; persona: indie dev shipping on MySQL, unit tests on SQLite
verdict: 7/10, would use it for this app, with the two data-integrity items fixed (F4, F13 below)
---

## Brief

Event booking app (venues, events with capacity, bookings, waiting list, atomic last-seat booking, promotion on cancel, attendee export as maps, monthly report via a stored procedure in a migration, bigint auto ids) in `/tmp/rango-blind-c/app` (lib `booking` + bin `run` with subcommands seed, queries, paging, crud, atomic, mistakes, json, concurrency, upsert_rv). It builds and runs on MySQL (default feature) and SQLite (`--no-default-features --features sqlite`, own `tests-sqlite/` migration set); MariaDB uses the MySQL build.

Blindness: only `docs/src/`, `README.md` and `cargo doc` output of my own app were read, plus compiler and runtime output. No source, tests, CHANGELOG, audit or git history. Compiler errors print repo paths; I did not open them. Nothing under the repo was modified. `BLOCKER: needed source` never needed. Friction was logged to `/tmp/rango-blind-c/.friction.md` as it happened. All test databases and roles (blind_c, blind_c_r, blind_c_p, blind_c_i, users app/mig) were dropped.

Measured, 100,000 bookings, 2,000 events, 20,000 attendees, 5,000 waitlist rows:

| Item | MySQL 8.4 | MariaDB 11.4 | SQLite |
|---|---|---|---|
| Seed 100k bookings | 1.0 s, ~97k rows/s (26 stmts total) | 0.70 s, ~142k rows/s | 0.28 s, ~355k rows/s (35 stmts) |
| Queries q1..q11 | all 1 stmt (prefetch M2M 2), 0.4-74 ms; slowest nearly_full raw 74 ms | same, nearly_full 60 ms | not timed |
| Keyset page 500 vs offset | 0.6 ms vs 5.6 ms (offset p1000 10.3 ms; offset order by booked_at no index 21.7 ms; keyset booked_at 13 ms); 500-page keyset walk 389 ms/500 stmts | 0.67 ms vs 4.9 ms | |
| EXPLAIN | booking_event_id_idx, booking_attendee_id_idx used; offset = PRIMARY scan 50k rows; starts_at/booked_at range = table scan until `#[field(index)]` | same, JSON plan format | |
| Add column to 100k table | 42 ms (instant DDL); index + drop column 262 ms | | |
| Last-seat race, 60 tasks, capacity 10 | 10 confirmed, 50 waitlisted, 361 stmts, 403 ms, no oversell | same, 124 ms | 9 confirmed, 51 errors "timed out" (F17) |
| Deadlock | `atomic_retry` recovered (2 attempts); `atomic` gives Err(Conflict) | same | no atomic_retry |
| dump / backup / restore | dump 28 MB 0.28 s; backup .json.gz 1.36 MB 0.37 s; restore 129,617 rows 1.9 s (MySQL), 0.57 s into SQLite (data checked, partial unique index recreated) | | |

## (a) Friction log

| # | Step | Severity | What happened | Expected | Exact error | Suggested fix |
|---|---|---|---|---|---|---|
| 1 | 6 | PAINFUL | `values()` on MySQL returns `NullText` for a non-null DATETIME(6); `raw_values` also returns `NullText` for DATETIME and for a computed/aliased DECIMAL. SQLite returns `Text("2026-01-01T04:00:00+00:00")` and `BigInt(50)` for the same data. Repro: `Booking::filter(&pool).eq(Booking::EVENT_ID,1i64).limit(3).values(&["id","booked_at","price_paid","seats"])` | the timestamp, or an error; the attendee-export-for-CSV use case (brief) loses the booking date silently | `{"booked_at": NullText, ...}` (no error) | Add date/time `SqlValue` variants or fail loudly; list them in values.md |
| 2 | 12 | PAINFUL | `rango import` CSV into an empty MySQL DB: 2,003 events with `venue_id` referencing 0 venues are stored although FK `event_venue_id_fkey` exists. Re-import prints the file row count although 0 were inserted | an FK error, or at least a statement that FKs are not checked | `✅ Imported 2003 row(s) from ../out/ev.csv` | Document in import.md; report inserted vs skipped; check FKs like restore does |
| 3 | 2/6 | PAINFUL | No guidance for running the same app code on MySQL in prod and SQLite in tests. Ops are per-backend and the prelude exposes one backend. I invented crate features each forwarding to one rango feature plus `use rango::mysql as db`. Code also diverges: no `atomic_retry`, no `FOR UPDATE`, different SqlValue variants, different constraint names | a documented pattern | n/a | A "test on SQLite, ship on MySQL" page with the cfg alias pattern and the list of differences |
| 4 | 6 | PAINFUL | Time types: `min::<OffsetDateTime>(Event::STARTS_AT)` works on MySQL, does not compile on SQLite; only Decimal is documented as excluded there | doc says so | `E0277: the trait bound for<'r> OffsetDateTime: sqlx_core::decode::Decode<'r, Sqlite> is not satisfied` | Add to aggregations.md the backend table for DateTime / Date / Time |
| 5 | 6 | PAINFUL | JSON output of models and time types is undocumented. Models and `SqlValue` are not `Serialize`; I derived `serde::Serialize` on the model by hand. Result: `{"venue_id":[2,null],"starts_at":"2026-01-01T04:00:00Z","doors_open":"18:30:00.0","categories":[[],null]}` (FK and M2M as tuples, Time as `18:30:00.0`) | a documented way to get plain JSON | `the trait bound rango::SqlValue: serde::Serialize is not satisfied` | Document the Serialize story and formats of Field types, or make FK/M2M serialize as id / list |
| 6 | 9 | PAINFUL | SQLite concurrency: 60 tasks calling `book()` give 51 failures displayed only as `timed out`; no doc on SQLite write concurrency, pool size or busy timeout; pool size/timeouts for MySQL also undocumented ("Connection pool (Postgres)" only) | a usable message and a recommended pattern for SQLite tests | `timed out` | Name the cause (pool acquire / busy) in the message; document pool settings per backend |
| 7 | 2 | DOC-WRONG | defining-models.md "Unsupported field types" still maps `DateTime<Utc>`, `NaiveDate`, `NaiveTime` (chrono) to Field types; field-types.md never shows how to build or read `FieldDateTime(OffsetDateTime)`, `FieldDate`, `FieldTime` and its date table has no MySQL column | 0.5.0 time crate types documented | n/a | Rewrite both tables for `time` |
| 8 | 1 | DOC-WRONG | cargo doc crate root says `rango-orm = "0.2"` (also in the sqlite/mysql variants), the book says 0.5 | 0.5 | n/a | Update the crate-level doc |
| 9 | 6 | DOC-WRONG | bulk.md says a row left unchanged counts 0 on MySQL/MariaDB. Re-upserting 10 identical rows returns `Ok(10)`; changed rows 20; new 3 (`bulk_upsert(&pool,&attendees,&["email"])` twice) | 0 for unchanged | `Ok(10)` | Fix the sentence (CLIENT_FOUND_ROWS) |
| 10 | 6 | MINOR | `bulk_upsert` with a Vec mixing id `Some` and `None` fails in SQL. The "all or none have ids" rule is documented for `bulk_create` only | an `Invalid` error before SQL | `Database { code: Some("21S01"), message: "Column count doesn't match value count at row 2" }` | Validate, and document for upsert |
| 11 | 11 | MINOR | Constraint errors differ by backend: FK on SQLite has no name (`ForeignKeyViolation { constraint: None }`), MySQL `Some("booking_event_id_fkey")`; unique name `attendee_email_unique` (MySQL) vs `attendee.email` (SQLite). Tests asserting names break across backends. `order_by` / `values` unknown column lack the "did you mean" of `filter` | consistent | `invalid: unknown column `nope` on table `attendee`` | Document, or normalize |
| 12 | 9 | MINOR | `atomic` deadlock returns `Err(Conflict)`, Display just `Conflict`; `is_retryable()` is true, which is good | message says deadlock | `Conflict` | Display `conflict (deadlock or serialization failure)` |
| 13 | 6 | MINOR | Partial unique on MySQL/MariaDB: two identical confirmed bookings both inserted (`Ok(())`, `Ok(())`); SQLite rejects the second. Documented as skipped, but the only signal is a warning at makemigrations | a runtime hint; a guard helper | `PARTIAL UNIQUE: first=Ok(()) second=Ok(())` | Show an app-level enforcement example (the app here uses FOR UPDATE) |
| 14 | 3 | MINOR | Least privilege: credentials.md has GRANTs for PostgreSQL only. MySQL: the DML-only role cannot run `rango migrate` (clear), `MIGRATE_DATABASE_URL` works, but a stored procedure from a migration is not callable by the app role | doc `GRANT EXECUTE` | `1370 (42000): execute command denied to user 'app'@'%' for routine 'blind_c_p.monthly_report'` | Add the MySQL GRANT block |
| 15 | 12 | MINOR | Restore into a non-empty DB gives a raw duplicate error; stored procedure is not replayed on restore (documented, note printed) | "target must be empty" hint | `1062 (23000): Duplicate entry '1' for key 'venue.PRIMARY'` | Add the hint |

Extra one-liners (not counted): `makemigrations --dry-run` assumes renames while the real non-TTY run aborts (the real message is clear); `mariadb://` prints "Connecting to database (mysql)"; the migrate.md URL priority list omits the rango.toml `url` that configuration.md lists; FieldInt is not Copy and no page shows reading Field values (`w.seats.0`); `insert` returns nanosecond FieldDateTime while the DB stores microseconds (documented); `select_related` emits `LIMIT 1000` by default; EXPLAIN ANALYZE is a tree on MySQL and JSON on MariaDB; connection-setup `SET` events show up in statement counts (documented in tracing.md, I filtered them).

## (b) Effort per step

| Step | Lines of code written | Calls or queries made |
|---|---|---|
| 1 install, init, config | Cargo.toml 25, rango.toml 3 | 1 build of the CLI |
| 2 models | 70 (models.rs) + 12 (lib.rs cfg aliases) | 0 |
| 3 makemigrations + migrate | 0 Rust, 13 SQL (procedure) | 3 migrations applied |
| 4 evolve | 8 SQL generated (rename, add column, check), SQLite set 26 SQL by hand | 1 rename needing `--yes` |
| 5 seed | ~90 in run.rs | 26 stmts (MySQL), 35 (SQLite) |
| 6 queries | ~110 ops.rs + ~80 run.rs | 1 stmt each, prefetch 2, get_or_create 2, update_or_create 3, add_many 3, bulk_update(500) 7, expire_waitlist 1 |
| 7 volume | ~40 | keyset walk 500 stmts; migration 42 ms |
| 8 transaction | ~20 | rollback leaves 0 rows (venues before 52, after 52) |
| 9 concurrency | ~60 | 361 stmts for 60 bookings |
| 10 observability | ~30 (tracing layer counting `sqlx::query`) | all steps |
| 11 mistakes | ~35 | 14 cases |
| 12 dump / restore / backup / export / import | 0 Rust | 7 CLI calls |

Total: 12 + 70 + 228 + 439 = 749 lines of Rust.

## (c) What was simple

- `#[rango::model(pk = "bigint")]`, `ForeignKey`, `ManyToMany`, constraints: first try, migrations generated correctly on all three backends.
- Stored procedure in a hand-written migration applied on MySQL and MariaDB; `raw::<_, MonthlyReport>` over `CALL` worked.
- Errors for unique, FK, check, unknown column (with suggestion), wrong value type and missing table are specific and good; TableMissing names the fix.
- Last-seat booking with `FOR UPDATE` plus `atomic_retry` never oversold; a real deadlock was retried.
- Query counts are exact (no N+1), keyset is 8-9 times faster than offset at page 500 here.
- dump/restore across backends (MySQL dump into SQLite) just worked, `MIGRATE_DATABASE_URL` just worked.
- `group_by`, `having`, `where_exists`, `in_subquery`, QuerySet update/delete in one statement.

## (d) Documentation pages wrong, missing or misleading

- `models/defining-models.md`, `models/field-types.md`: chrono leftovers, no `time` usage, no MySQL column in the date table.
- cargo doc crate root: version 0.2.
- `querying/values.md`: SqlValue list has no date/time variant and says nothing of the NullText results on MySQL.
- `querying/bulk.md`: unchanged-row count on MySQL wrong; upsert id rule missing.
- `querying/aggregations.md`: OffsetDateTime min/max not listed as SQLite-incompatible.
- `cli/import.md`: no word on FK checks or counts.
- `backends/sqlite.md` / missing page: running one app on MySQL and SQLite; SQLite concurrency; pool settings for MySQL and SQLite (`getting-started/configuration.md` covers Postgres only).
- `security/credentials.md`: no MySQL GRANT block (incl. EXECUTE).
- Missing: JSON serialization of models and time types.

## (e) Verdict

Score 7/10. I would use Rango for this app on MySQL: schema, migrations, atomic booking, retry, bulk speed and error messages are good, and the volume numbers are fine. What holds it back is the "ship on MySQL, test on SQLite" story: the two backends diverge in types (values, aggregates), errors, concurrency and retry, and nothing documents it, so the SQLite tests prove less than they appear to. The silent `NullText` for dates in `values()` / `raw_values` and the FK-blind CSV import are the two items that can lose or corrupt data without any error.

Counts: BLOCKER 0, PAINFUL 6, DOC-WRONG 3, MINOR 6 (15 findings).

## (f) Decisions

Checked against develop at commit 6230602 (2026-11-23).

| # | Decision | Commit or reason |
|---|---|---|
| 1 | fixed | 6230602: MySQL / MariaDB and PostgreSQL decode DATETIME, DATE, TIME, JSON and computed DECIMAL in `values()`, `aggregate()` and `raw_values`; a non-null value of an unreadable type is `Error::Decode`, never a null. Tested on PostgreSQL, MySQL 8.4 and MariaDB 11.4. |
| 2 | fixed in f698ba6 | On MySQL / MariaDB, import and restore check every foreign key before commit and write nothing if a row is orphaned; the summary reports new and already-present rows. |
| 3 | open | Same as A 5. |
| 4 | fixed | b06601a: same root cause as A 2. |
| 5 | open | Same as A 4; `Time` serializing as `18:30:00.0` to check against the documented format. |
| 6 | open | Same as A 12; also document pool settings per backend. |
| 7 | fixed | b06601a: both tables list the `time` types; field-types.md shows how to build and read them. |
| 8 | fixed | b06601a: the crate root says 0.5. |
| 9 | fixed | b06601a: bulk.md says an unchanged row counts 1 (sqlx connects with `CLIENT_FOUND_ROWS`). |
| 10 | open | Same as A 7. |
| 11 | open | Same as A 13; add the "did you mean" hint to `order_by` and `values`. |
| 12 | open | Display `conflict (deadlock or serialization failure)`. |
| 13 | open | Show app-level enforcement of a partial unique rule on MySQL / MariaDB. |
| 14 | open | Add the MySQL GRANT block, including `GRANT EXECUTE` for routines. |
| 15 | open | Hint that the restore target must be empty on a duplicate key. |
