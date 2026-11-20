---
date: 2026-11-20
commit: 7e0d1ec
kind: blind-campaign-c
tool: Rango (rango-orm 0.2.0 path dependency, CLI built from repo), MySQL 8.4.13, MariaDB 11.4.13, SQLite (bundled, in-memory)
model: Sonnet 5.5 (claude-sonnet-5-5)
scope: event booking app, indie developer on MySQL, tests on SQLite; all 12 steps and the campaign C feature list
verdict: 8/10, would use it for this app
---

## Brief

Persona: indie developer shipping on MySQL, SQLite for fast tests, pragmatic, not an SQL expert. App: venues, events with capacity, bookings (100,000 rows), waiting list, atomic last-seat booking with promotion on cancel, attendee export as plain maps, monthly report via a stored procedure created in a migration, bigint auto-increment ids.

Blindness: I read only `docs/src/` and `README.md` under the repo. I never opened source, tests, examples, AGENTS.md, CHANGELOG.md, audit/ or git history. A few compiler errors printed repo paths (`rango-orm-core/src/filter.rs`); I did not follow them. Rango was a path dependency, the CLI came from `cargo install --path`. Real servers only: MySQL 8.4.13 (:3307), MariaDB 11.4.13 (:3308), SQLite. Work dir `/tmp/rango-blind-c` (crate `booking`: `src/models.rs`, `src/lib.rs`, `src/bin/app.rs`, `tests/booking.rs`, `tests-sqlite/`). Tests pass on all three: 8 on MySQL, 8 on MariaDB, 5 on SQLite (the concurrency, deadlock and forgot-migrate tests are MySQL/MariaDB only). All my databases and the two roles were dropped at the end.

Numbers (release build, local Docker, 100,000 bookings, 1,000 events, 20,000 attendees, 50 venues, 3,000 pivot rows):

| Measure | MySQL 8.4 | MariaDB 11.4 |
|---|---|---|
| `bulk_create` of 100k bookings | 0.99 s = 101k rows/s (10 statements) | 0.47 s = 213k rows/s (12 statements) |
| Whole seed (incl. 1,000 `add_many` calls) | 2.7 s | 1.8 s |
| `ALTER TABLE booking ADD COLUMN ... NOT NULL DEFAULT 0` via `rango migrate` | 0.02 s | not timed on a full table |
| `rango dump` (27 MB JSON) | 0.26 s | n/a |
| `rango backup` (1.1 MB gz) | 0.31 s | n/a |
| `rango restore` 124k rows into empty DB | 56.7 s | 51.8 s (SQLite target: 8.7 s) |
| `rango import` 100k rows | 44 s | n/a |
| Offset page 500 (limit 100 offset 49900) / keyset `after(id)` | 4.8 ms / 1.3 ms | 4.9 ms / 1.2 ms |
| 500 keyset pages walked | 343 ms | 320 ms |

`.explain_analyze()` showed `Index lookup on booking using booking_event_id_idx` for `event_id = 5` on MySQL, `access_type: ref` on MariaDB. A filter on `status` (no index) does a full table scan, as expected. Nothing odd appeared only at volume, apart from restore/import speed (F8).

## (a) Friction log

| # | Step | Severity | What happened | Expected | Exact error | Suggested fix |
|---|---|---|---|---|---|---|
| 1 | 6 stored procedure | PAINFUL | `CALL monthly_report(?, ?)` returns columns that are not a model. Docs only show `raw::<_, User>(...)`. I looked for a raw-to-maps function (`rango::mysql::raw_values`, guessed) and found none. Workaround: an unmanaged model `#[model(managed = false, table = "monthly_report")]` with `#[field(primary_key)] event_id`, and `FieldDecimal<20, 0>` because `SUM()` returns DECIMAL. | A documented way to read an ad-hoc result set (maps or tuples). | `error[E0425]: cannot find function `raw_values` in module `rango::mysql`` | In raw-sql.md: say that a procedure or report result is read through a `managed = false` model, with the SUM/DECIMAL typing gotcha, or add `raw_maps`. |
| 2 | 12 restore and import | PAINFUL | `rango restore` of 124,076 rows took 56.7 s (MySQL), 51.8 s (MariaDB); `rango import` of 100,000 bookings 44 s. The same rows go in with `bulk_create` in 1 s. The process is mostly idle (user 3 s, sys 5 s). Nothing warns about this. | A restore of a backup taken in 0.3 s in seconds, or a doc note with the expected rate. | none | Batch the inserts in restore/import (multi-row `INSERT`), or document the rate (about 2,000 rows/s on MySQL) in dump.md. |
| 3 | 5 seed | PAINFUL | `bulk_create` returns only the row count, no ids. Auto-increment ids are not returned, so to link M2M tags I had to re-read the rows (`Tag::filter().like(...)`). `insert` does return the id. | A documented way to get the ids of a bulk insert on MySQL (no `RETURNING`). | none | In bulk.md and mysql.md: "ids are not returned; re-read by a natural key". |
| 4 | 11 wrong type | DOC-WRONG | filter.md says string column names "accept any value and are checked by the database at runtime". On MySQL and MariaDB `Booking::filter(&pool).eq("seats", "lots").all()` returns `Ok(vec![])` (the server coerces 'lots' to 0, in strict mode too). On SQLite also `Ok`. Only a Postgres-like server errors. | Either an error, or a doc that says MySQL, MariaDB and SQLite silently coerce. | `WRONGTYPE: None` (no error) | State it in filter.md, and recommend typed columns (`Booking::SEATS`), which fail at compile time with a clear message. |
| 5 | 2 relations | DOC-WRONG | relations.md, FK section: "The column is stored as `UUID` (Postgres) or `TEXT` (SQLite)", right after an example that says `BIGINT NOT NULL`. On MySQL my FK columns are `BIGINT`. The sentence is stale and contradicts the page. | One consistent description: the type follows the target key; MySQL `BINARY(16)` / `BIGINT`. | none | Replace the sentence with the type table that is in mysql.md. |
| 6 | 2 constraints | MINOR | The partial unique index `uniq_active_booking` (one confirmed booking per attendee per event) is skipped on MySQL/MariaDB and enforced on SQLite. A second confirmed booking of the same attendee is `ACCEPTED` on MySQL and MariaDB, and `UniqueViolation { constraint: Some("booking.event_id, booking.attendee_id") }` on SQLite, so the SQLite tests pass where production would not fail. `makemigrations` prints nothing on stdout; the skip is only an SQL comment in the migration. Documented in constraints.md and mysql.md, but easy to miss. | A warning at `makemigrations` time. | `-- MySQL/MariaDB do not support partial unique indexes: constraint "uniq_active_booking" on "booking" was skipped (enforce it in application code).` (SQL comment only) | Print a `warning:` line on stdout when a constraint is skipped. Mention in sqlite.md that tests will enforce what MySQL does not. |
| 7 | 6 aggregates | MINOR | `having(alias, Cmp, value)` takes a constant only. "Events above 90% of capacity" needs `capacity` from the events table, so I did group-by on bookings plus a second query on events and compared in Rust. Not documented as a limit. | A line in aggregations.md telling that `having` compares to a constant and cannot reference another table; use `raw` or two queries. | none | Doc note, or a `raw`-based recipe. |
| 8 | 10 tracing | MINOR | `prefetch_related` opens one `db.query` span but runs 2 SQL statements; the sqlx events are the only exact count. A fresh pool also logs 2 `SET sql_mode ...` statements on its first connection, which count as `sqlx::query` events. Counting by span undercounts (1 instead of 2), counting by sqlx events overcounts by 2 at start. | Tracing page tells which one to count. | none | In tracing.md: "count `sqlx::query` events for statements; one span can cover several; connection setup statements appear on each new connection". |
| 9 | 9 retry | MINOR | `atomic_retry` is missing under the sqlite feature. The doc sentence is garbled: "Postgres and MySQL/MariaDB (`atomic()` works on every backend, SQLite has no equivalent)". Sharing code between MySQL prod and SQLite tests needed a `macro_rules!` with `#[cfg]`. | A clear sentence: "`atomic_retry` exists on Postgres and MySQL/MariaDB only; use `atomic` on SQLite". | `error[E0425]: cannot find function `atomic_retry` in this scope` | Fix the sentence in transactions.md; maybe provide `atomic_retry` on SQLite as a plain `atomic`. |
| 10 | 12 restore | MINOR | The restore warning lists all 6 migrations "recorded as applied without running them", though only `0005_monthly_report.sql` holds hand-written SQL. After restore the procedure is gone and `rango migrate` says nothing is pending. | Restore says which files hold SQL that the snapshot cannot rebuild, or `migrate` has a way to replay one. | `warning: the schema was rebuilt from the dump's snapshot and these migrations were recorded as applied without running them: 0001_auto.sql, ..., 0006_booking.sql. Any custom SQL in them ... was NOT reproduced` | Warn only for files that are not the plain output of makemigrations (or document a `rango migrate --replay <file>`). |
| 11 | 12 export | MINOR | `rango export --format csv` writes columns in alphabetical order (`email,full_name,id`), `id` not first, which is poor for an organizer's CSV. Bools are `0`/`1` from SQLite and `true`/`false` from MySQL for the same data. Piping to `head` panics. | Schema column order, one bool format. | `thread 'main' panicked at .../std/src/io/stdio.rs:1166:9: failed printing to stdout: Broken pipe (os error 32)` | Keep schema order; handle `EPIPE`; document the bool format. |
| 12 | 2 least privilege | MINOR | credentials.md gives only PostgreSQL `GRANT`s. I wrote the MySQL ones by hand (`GRANT ALL` to the migration role, `SELECT,INSERT,UPDATE,DELETE` to the app role). `rango migrate` with the app role fails even with nothing pending because it runs `CREATE TABLE IF NOT EXISTS _rango_migrations`. The doc's "dump needs the migration role" is not true on MySQL: with plain DML grants `rango dump` worked. | A MySQL grant example, and the note that the app role cannot run `migrate` at all. | `error returned from database: 1142 (42000): CREATE command denied to user 'app'@'172.23.0.1' for table '_rango_migrations'` | Add a MySQL block to credentials.md. |
| 13 | 1 and 2 | MINOR | field-types.md tables for primitive, text, date and other types have only Postgres and SQLite columns. MySQL columns are in mysql.md only. Also the migrate.md tracking table SQL has no MySQL version. | One place with all three backends. | none | Add the MySQL column to the tables or link mysql.md at the top. |
| 14 | 2 TLS | MINOR | With no `ssl-mode` on loopback, the connection was in fact TLS (`Ssl_cipher = TLS_AES_256_GCM_SHA384`); docs say loopback is "left as is", which does not say whether that means PREFERRED or DISABLED. `?ssl-mode=VERIFY_IDENTITY` against the Docker server fails clearly. | One sentence on the loopback default. | `Connection("error communicating with database: invalid peer certificate: UnknownIssuer")` | State "loopback uses the driver default (PREFERRED)". |
| 15 | 6 | MINOR | crud.md and transactions.md still describe `get_or_create` as `INSERT ... ON CONFLICT DO NOTHING RETURNING *`; on MySQL it is another statement (mysql.md has it right). | Backend note on the same page. | none | Add "(MySQL: see backends/mysql.md)". |

## (b) Effort per step

SQL counts are the `sqlx::query` events seen by a small tracing layer I wrote (about 15 lines). Lines are Rust lines I wrote for the step (whole project 613 lines: lib 137, models 67, app 202, tests 191, misc 16).

| Step | Lines of code written | Calls or queries made |
|---|---|---|
| 1 install, init, config | 0 (3 CLI commands, 1 `Cargo.toml` of 25 lines) | 0 |
| 2 models (6 models, 1 M2M, 3 constraints, 1 partial unique) | 67 | 0 |
| 3 makemigrations and migrate | 0 | 1 migration, 6 statements |
| 4 evolve (add field, rename, check constraint; then add column on big table) | 3 lines of model edits, 1 hand-written migration (procedure, 15 SQL lines) | 4 migrations (`--yes` needed for the rename without a terminal) |
| 5 seed (100k bookings, 20k attendees, 1k events, 3k pivot) | about 70 (in `app.rs`) | venues 1, events 2, attendees 1, bookings 10 (MySQL) / 12 (MariaDB), `add_many` 3,002 for 1,000 events (3 per call: BEGIN, INSERT, COMMIT) |
| 6 real queries (join, prefetch, aggregates, group by, subquery, export, procedure) | about 100 (lib + app) | select_related 1, prefetch_related 2 (+2 connection setup on first use), count 1, sum+max 2, group by 1, nearly_full 2, where_exists/or/not 2, export 1, procedure 1 |
| 7 volume | 20 | offset page 500: 1, keyset page: 1, 500-page keyset walk: 500 (1 per page, no N+1) |
| 8 transaction (book last seat, cancel, promote) | about 60 (lib) | `book` = BEGIN, SELECT ... FOR UPDATE, SUM, INSERT, COMMIT |
| 9 concurrency and retry (30 concurrent bookings on 5 seats, forced deadlock) | 40 (tests) | 5 confirmed exactly; the opposite-order deadlock was retried once (3 attempts, both Ok) on MySQL and MariaDB |
| 10 observability | 15 (counter layer) | see above |
| 11 deliberate mistakes | about 45 | 12 error cases, all judged below |
| 12 dump, restore, backup, export, import | 0 (CLI) | dump 1, backup 1, restore 3 (MySQL, MariaDB, SQLite), import 1 |

Error messages (all judged as a user would):
- Forgot `migrate`: `TableMissing { table: "venue" }` on both servers. Good.
- Unique violation: `UniqueViolation { constraint: Some("attendee_email_unique") }` (MySQL, MariaDB), `Some("attendee.email")` (SQLite). Good.
- FK to a missing row: `ForeignKeyViolation { constraint: Some("booking_event_id_fkey") }`; SQLite `constraint: None`. Good, difference documented.
- Check violation: `CheckViolation { constraint: Some("chk_seats") }`. Good.
- Unknown column as a string: ``Invalid("unknown column `evnt_id` on table `booking`, did you mean `event_id`?")``. Very good.
- Wrong type: a typed column gives a compile error (`is not a column of this model that accepts a `&str` value`, with a note on what to pass). A runtime wrong type through `raw_execute` gives `Database { code: Some("HY000"), message: "Incorrect integer value: 'abc' for column 'seats' at row 1" }`. A string name with a wrong value is silent (finding 4).
- Value too long: `FieldError("Value too long (max 5)")`: does not name the field.

## (c) What was simple

- `#[rango::model(pk = "bigint")]` plus `rango init --backend mysql`, `makemigrations`, `migrate`: models to a working schema in a few minutes, on MySQL and MariaDB with the same migration files.
- The rename prompt, and the clear refusal without a terminal (error names `--yes`). Add-column on a NOT NULL field says to add `#[field(default = ...)]`.
- The query builder: `where_exists`, `in_subquery`, `group_by` with `annotate_count`, `values`, `select_related`, `prefetch_related` and keyset `after()` all worked the first time I wrote them from the docs, with the documented SQL.
- Atomic booking: `atomic_retry` with `raw_scalar("... FOR UPDATE")`, then `insert`: no oversell with 30 concurrent tasks, deadlock retried transparently. M2M `add_many`, `QuerySet delete` of stale waitlist rows (one statement), `bulk_upsert`/`bulk_update` on MySQL.
- Typed errors with constraint names; compile errors for wrong column types are readable.
- `rango dump/backup` are fast, and a MySQL dump restores into MariaDB and SQLite.

## (d) Documentation pages wrong, missing or misleading

- `querying/filter.md`: claims string names are "checked by the database at runtime"; false for a wrong value type on MySQL, MariaDB, SQLite (finding 4). DOC-WRONG.
- `models/relations.md`: stale "stored as UUID (Postgres) or TEXT (SQLite)" sentence (finding 5). DOC-WRONG.
- `querying/raw-sql.md`: no way to read a result set that is not a model (finding 1). Missing.
- `operations/bulk.md`, `backends/mysql.md`: ids of bulk-created rows are not returned (finding 3). Missing.
- `cli/dump.md`: restore and import speed (finding 2) and what the migration warning means (finding 10).
- `operations/transactions.md`: garbled sentence on `atomic_retry` and SQLite (finding 9).
- `security/credentials.md`: PostgreSQL only; no MySQL grants (finding 12).
- `models/field-types.md`, `migrations/migrate.md`: PostgreSQL and SQLite only (finding 13).
- `operations/tracing.md`: which count to trust (finding 8). `querying/aggregations.md`: `having` limit (finding 7).
- `operations/crud.md`: `get_or_create` described for Postgres only (finding 15).

## (e) Verdict

Score: 8/10. No BLOCKER. Everything in the campaign list worked on MySQL 8.4, MariaDB 11.4 and SQLite from the docs alone, in about 600 lines. The app did what the brief asked: atomic last seat, waiting-list promotion, plain-map export, procedure from a migration, group by / having, queryset update and delete, add_many, TLS, least-privilege roles, dump/restore across the three engines.

I lose points for restore and import running at about 2,000 rows/s (finding 2), the stored procedure result needing a fake model (finding 1), the silent difference between SQLite tests and MySQL for the partial unique rule (finding 6), and a few stale doc sentences.

Would I use Rango for this app: yes. I would keep the "one confirmed booking per attendee" rule in application code (or a generated column and plain unique index), and I would not rely on `rango restore` for a large backup until it is faster.

## (f) Decisions

Checked against the tree at commit 86a9183 (2026-11-20).

| # | Decision | Commit or reason |
|---|---|---|
| 1 | documented | be190cb: raw-sql.md shows the unmanaged-model pattern for result sets that are not a model. A raw function returning maps stays open. |
| 2 | fixed | afca4bf: multi-row inserts in restore and import (see A #11). |
| 3 | documented | be190cb: bulk.md says ids are not returned and how to get them (keys generated in Rust, or re-read by natural key). |
| 4 | fixed | be190cb: filter.md states that MySQL, MariaDB and SQLite coerce silently. The value check stays open (A #4). |
| 5 | fixed | be190cb: relations.md, the FK type follows the target key, MySQL included. |
| 6 | open | `makemigrations` should warn on stderr when a partial unique index is skipped on MySQL / MariaDB. |
| 7 | documented | be190cb: aggregations.md, `having` compares to a constant; raw SQL or two queries otherwise. |
| 8 | fixed | be190cb: tracing.md, "Counting statements". |
| 9 | fixed | be190cb: transactions.md sentence rewritten. No `atomic_retry` on SQLite: there is no retryable conflict error to catch. |
| 10 | documented | be190cb: same as A #10. |
| 11 | fixed | 68d43e4: CSV columns in table order, broken pipe exits quietly. Booleans follow each backend's storage (SQLite has no boolean type): not changed. |
| 12 | fixed | be190cb: credentials.md has MySQL grants and the note on `migrate` with the app role. |
| 13 | fixed | be190cb: field-types.md links the MySQL type table; migrate.md has the MySQL tracking table. |
| 14 | fixed | be190cb: credentials.md, loopback uses the driver default (`prefer` / `PREFERRED`). |
| 15 | fixed | be190cb: crud.md and transactions.md give the MySQL `get_or_create` strategy. |
