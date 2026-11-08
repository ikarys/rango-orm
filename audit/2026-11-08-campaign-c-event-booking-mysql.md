# Blind test report, campaign C: event booking on MySQL 8.4 / MariaDB 11.4 / SQLite

- Date: 2026-11-08
- Rango commit: `bd5de84` (path dependency, `rango_version` in dumps: 0.2.0-beta.1)
- Databases: MySQL 8.4 (port 3307), MariaDB 11.4 (port 3308), SQLite (bundled, temp file)
- Persona: indie developer shipping on MySQL, unit tests on SQLite, pragmatic, impatient with setup
- Tester model: Claude Sonnet 5.5
- App: `/tmp/rango-blind-c/booking` (lib: models + ops; bin `demo` runs every step on MySQL/MariaDB; `tests/sqlite.rs` runs on SQLite). The demo databases were dropped at the end; `cargo build` and `cargo test --no-default-features --features sqlite --test sqlite` pass.

## (a) Friction log

| # | Step | Severity | What happened | Expected | Exact error | Suggested fix |
|---|---|---|---|---|---|---|
| 1 | 6 values() | PAINFUL | `Event::filter(pool).limit(3).values(&["id","title","price"])` returns `price` (DECIMAL(10,2)) as `NullText` on MySQL and MariaDB (silent data loss), `BigInt(15)` for 15.00 on SQLite (scale lost). No error. | a decimal value, same on all backends | none (silent) | Decode DECIMAL as `SqlValue::Decimal`/text on all backends; document the mapping |
| 2 | 6 raw | DOC-WRONG | raw-sql.md shows `raw::<User>(...)` with one generic; real signature needs two | docs compile | `error[E0107]: function takes 2 generic arguments but 1 generic argument was supplied` (fix: `raw::<_, Event>(..)`) | Fix the examples |
| 3 | 8 concurrency | PAINFUL | `atomic_retry` future is not `Send`: `tokio::spawn(book_seat(..))` using it fails; the same code with `atomic` compiles. transactions.md promises `Send` only for `atomic`. Worked around with `LocalSet`/`spawn_local`. | spawnable retry loop (a booking service is multi-task) | `implementation of `Send` is not general enough` | Make `atomic_retry` `Send` or state clearly it is not, with the workaround |
| 4 | 2 models/ops | PAINFUL | `FieldVarchar::new(..)?` inside a Rango closure or fn returning `rango::Result` does not compile: no `From<FieldError> for rango::Error`. Every field constructor needs a hand-written mapper. | `?` works | `error[E0277]: `?` couldn't convert the error to `rango::Error`` | `impl From<FieldError> for Error` (Invalid) |
| 5 | 3 makemigrations | DOC-WRONG | migration files are `0001_auto.sql`, `0002_event.sql`, ... while makemigrations.md describes `NNNN_<tables>.sql` | docs match the output | n/a | Document the real naming rule (`auto` when more than one table) |
| 6 | 1 configuration | MINOR | configuration.md and init.md list only postgres and sqlite for `backend`/URL schemes; `MIGRATE_DATABASE_URL` priority is only in migrate.md. `slow_query_threshold_ms` text in configuration.md says only SQLite is fixed at 500 ms, tracing.md says SQLite and MySQL | consistent docs | n/a | Update both pages |
| 7 | 10 errors | PAINFUL | On MySQL 8.4 and MariaDB 11.4 `UniqueViolation`, `ForeignKeyViolation` and `CheckViolation` all carry `constraint: None`, although the server message names the key (`for key 'attendee.attendee_email_unique'`). SQLite fills it (`Some("attendee.email")`, `Some("chk_event_capacity")`). errors.md says the name is held when the backend reports it. Display is just "unique violation", so logs tell nothing. | the constraint name, as on SQLite | `UniqueViolation { constraint: None }` | Parse the name from MySQL error 1062/1451/1452/3819 |
| 8 | 2 SQLite port | PAINFUL | `rango.toml` fixes one backend per project; migrations are dialect-specific (backticks and `BINARY(16)` vs quotes and `TEXT`; partial unique index emitted on SQLite, skipped on MySQL); `makemigrations` has no `--backend` flag. I made a second project dir (`booking/sqlite/` with its own `rango.toml` and snapshot) and ran `rango makemigrations ../src`. Hand-written migrations (procedure) must be left out. Nothing documents the "prod on MySQL, tests on SQLite" workflow. | one documented recipe | n/a | Add a doc section and/or `makemigrations --backend` |
| 9 | 2 SQLite tests | PAINFUL | sqlite.md "Testing with in-memory SQLite" uses hand-written `CREATE TABLE` and `sqlx::query` (sqlx is not a dependency of the user). No documented library API to apply generated migrations in a test. I split `0001_auto.sql` on `";\n"` and used `raw_execute`. | `rango::migrate(&pool, "migrations")` or equivalent | n/a | Expose and document a migrate function |
| 10 | 6 aggregates | PAINFUL | `Event::filter(pool).max::<Decimal>("price")` compiles and works on MySQL/MariaDB but not on SQLite. The shared app code is not portable. `sum(Event::PRICE)` is `Decimal` on MySQL and `Some(5.0)` (float) on SQLite. | same code on all backends, or a documented rule | `the trait bound `rango::Decimal: sqlx_core::types::Type<Sqlite>` is not satisfied` | Document decimal aggregates per backend |
| 11 | 11 restore | PAINFUL | `rango restore` of a MySQL dump into an empty MySQL DB restores 2355 rows and records `0006_monthly_report_procedure.sql` as applied, but the stored procedure does not exist. `rango migrate` then says it has nothing to apply. | restore either replays non-snapshot SQL or warns | `ERROR 1305 (42000): PROCEDURE monthly_report does not exist`; `Nothing to apply — database is up to date.` | Warn in dump.md and at restore time when a migration contains objects outside the snapshot |
| 12 | 11 restore | PAINFUL | `rango migrate` on a fresh SQLite file followed by `rango restore m.json` fails. Restore re-inserts the migration names. Restore alone (no migrate) works. | either order works, or a clear message | `UNIQUE constraint failed: _rango_migrations.name` | Skip existing migration rows, or say "restore into a database with no tables" |
| 13 | 10 errors | MINOR | Wrong-type value through a string column name returns no error on MySQL: `.eq("capacity", "many")` gives `Ok(0)`, `.eq("title", 5_i64)` gives `Ok(0)` | an error like the typed column compile error | none | Document: string names accept any value and rely on server coercion |
| 14 | 10 errors | MINOR | `bulk_update(&[], &["nope"])` returns `Ok`; unknown column is not checked when the slice is empty (docs: columns checked before any SQL) | `Error::Invalid` | none | Validate columns before the empty shortcut |
| 15 | 8 bulk | MINOR | `bulk_upsert` of 8 rows (5 updated, 3 new) returns 13 on MySQL; the meaning of the number is undocumented (MySQL counts 2 per updated row) | documented value | none | Document the return value per backend |
| 16 | 6 queries | MINOR | `.all()` always returns `WithRelated<M>`, even with no relation; reuse needs `(**row).clone()`. M2M `set` rejects `&[WithRelated<Category>]` | plain `Vec<M>` when no relation requested | `expected `&[Category]`, found `&[WithRelated<Category>]`` | Show the deref idiom in the docs |
| 17 | 2 models | MINOR | `#[rango::model]` does not derive `Debug`/`Clone`; docs are silent, `{:?}` of an outcome fails | mention in docs | `` `Venue` doesn't implement `Debug` `` | Document `#[derive(Debug, Clone)]` next to the attribute |
| 18 | 6 raw | MINOR | `raw` only returns model rows, so reading a stored procedure result set needs an unmanaged model (`managed = false`) | a documented recipe | n/a | Add the recipe to raw-sql.md |
| 19 | 4 makemigrations | MINOR | A raw `bool` field gives a helpful error from the derive (`use FieldBool`) but `rango makemigrations` says only `Unknown field type: bool` | same hint | `Error: Field errors: src/models.rs: Event: Unknown field type: bool` | Reuse the derive message |
| 20 | 10 errors | MINOR | Decimal overflow surfaces as a raw database error (no typed variant) | typed error or documented | `Database { code: Some("22003"), message: "Out of range value for column 'price' at row 1" }` | Document or add a variant |
| 21 | 9 observability | MINOR | One `SELECT SLEEP(0.7)` through `raw_scalar` produced 2 WARN events on `sqlx::query` in my counter (`slow_warns=2`). Not understood (duplicate event or span plus event) | one WARN | none | Check whether the slow warning is emitted twice |
| 22 | 5 seed | MINOR | `rango::raw_execute` is not at the crate root when only the `sqlite`/`mysql` features are on (`rustc` note: gated behind `postgres`); it is in the prelude. The docs show `raw_execute` unqualified so this is only a trap for full paths | n/a | `error[E0425]: cannot find function `raw_execute` in crate `rango`` | Ungate the root re-export or leave |

## (b) Per step: lines of Rust and SQL queries

Measured with a tracing layer counting `sqlx::query` events. MySQL and MariaDB gave identical counts.

| Step | Rust lines | SQL queries |
|---|---|---|
| 1 install, init, configure | 0 (rango.toml 4 lines, Cargo.toml 20) | 0 |
| 2 models (6 models, 1 unmanaged) | 73 (models.rs) | 0 |
| 3 makemigrations + migrate | 0 | 7 migration files, `migrate` applied them on both servers |
| 4 evolve (add field, rename 2 fields, 3 constraints, 1 more field) | 3 | 0 (migrations 0002 to 0005, 0007) |
| 5 seed (20 venues, 300 events, 2000 attendees) | about 40 | 1 per `bulk_create` (3 total) |
| 6 queries: or/not/group, in_subquery, where_exists, sum/avg/min/max, select_related, prefetch, offset page, keyset (2000 rows, pages of 500), values | about 60 | 1 each; prefetch 2; keyset 5; M2M add x3 = 3, all = 1, set = 4 |
| 7 transaction: book_seat and cancel with waiting list promotion | about 70 (ops.rs 173 with helpers) | cancel and promote: 7 |
| 8 concurrency: 20 tasks for 5 seats; forced deadlock | about 30 | 136 queries; 5 confirmed, 15 waitlisted, no oversell. Deadlock: 12 queries, 3 attempts, both succeeded |
| 9 observability | 37 (obs.rs) | counter works; slow WARN fires at 500 ms |
| 10 deliberate mistakes | about 50 | see table above |
| 11 dump, restore, backup | 0 (CLI) | `dump` 0.13 s for 2355 rows; backup 20 KB gz, `0600` |
| get_or_create / update_or_create | n/a | new 1, existing 2; update_or_create 3 |
| bulk_update 301 rows | n/a | 6 (chunks of 100, begin, commit) |
| stored procedure `CALL monthly_report` | 12 | 1 (301 rows) |

SQLite: the same ops ran on a temp file (2 tests): booking, waitlist, cancel and promote, partial unique index, error variants. `FOR UPDATE` and `atomic_retry` are replaced by a cfg-switched `txn!` macro in `ops.rs`.

Differences between the three backends:

| Topic | MySQL 8.4 | MariaDB 11.4 | SQLite |
|---|---|---|---|
| `RETURNING` | none (insert re-selects) | none | yes |
| partial unique index | skipped with a comment (checked in `book_seat` by hand) | same | `CREATE UNIQUE INDEX ... WHERE status = 'confirmed'`, enforced: `UniqueViolation { constraint: Some("booking.event_id, booking.attendee_id") }` |
| error `constraint` | None | None | Some for unique and check, None for FK |
| `values()` decimal | NullText | NullText | BigInt(15) |
| aggregate decimal | `Decimal` | `Decimal` | not decodable as `Decimal` (compile error) |
| `explain_analyze` | text plan | JSON | `EXPLAIN QUERY PLAN` |
| `atomic_retry`, `FOR UPDATE` | yes | yes | not available |
| stored procedure | yes | yes | no |
| TLS | `ssl-mode=REQUIRED` works; `VERIFY_IDENTITY` fails with `invalid peer certificate: UnknownIssuer` (self-signed, expected); `DISABLED` works | not tried | n/a |
| least-privilege | DML-only role: `CREATE command denied to user 'app'@'...' for table '_rango_migrations'`; with `MIGRATE_DATABASE_URL` the migration role is used and it works | not tried | n/a |
| dump restore | works into empty MySQL, MariaDB (exact decimals `10.25`) and SQLite (partial unique index recreated); `price` 10.00 becomes `integer 10` in SQLite | works | works if no prior `migrate` |

## (c) What felt simple

- `#[rango::model(pk = "bigint")]` plus `makemigrations` gave a working schema on three backends without writing SQL.
- Rename detection prompts and its error text when not interactive are clear; hand-written migrations (the procedure) coexist with generated ones, and `makemigrations --check` ignores them.
- Query counts are minimal: relations, subqueries and aggregates cost one statement.
- `get_or_create`, `update_or_create`, bulk ops and `atomic` read well; real deadlock recovery with `atomic_retry` worked first time.
- Errors on typed columns are compile errors with a good hint; unknown string columns give "did you mean".
- `dump` / `restore` / `backup` work across backends in one command each.

## (d) Doc pages wrong, missing or misleading

- querying/raw-sql.md: `raw::<User>` generic count wrong; no recipe for reading a stored procedure result set.
- operations/transactions.md: `Send` promised for `atomic` only; `atomic_retry` is not spawnable; no word on it.
- cli/makemigrations.md: file naming rule does not match `0001_auto.sql`.
- getting-started/configuration.md and cli/init.md: only two backends listed; MySQL missing; slow threshold text disagrees with operations/tracing.md.
- backends/sqlite.md: testing section hand-writes tables; no way to run generated migrations. No page for "MySQL in prod, SQLite in tests".
- errors page: `constraint` is documented as filled when the backend reports it; MySQL and MariaDB do not.
- cli/dump.md: restore does not replay hand-written migration objects; order restore/migrate undocumented.
- querying aggregation/values pages: decimal behaviour per backend missing.
- bulk ops page: `bulk_upsert` return value on MySQL undocumented.

## (e) Verdict

Score: 6.5/10.

Would I use Rango for this app? Yes for the MySQL production path: schema, migrations, concurrency, transactions and the query builder were pleasant and fast. I would not rely on it yet for the SQLite test double: decimals diverge (`values()`, aggregates), the constraint names in errors differ, `FOR UPDATE`/`atomic_retry` have no SQLite equivalent so tests exercise different code, and there is no documented way to apply generated migrations in tests. The silent `NullText` for a DECIMAL column in `values()` is the finding I would fix first, because it loses data with no error, in the exact feature (plain maps for CSV export) that the brief asks for.
