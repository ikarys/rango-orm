# Blind test report, campaign A (library, Django developer)

- Date: 2026-11-08
- Rango commit: `bd5de84` (path dependency, pre-release 0.2)
- Databases: SQLite 3.46 (bundled by sqlx, file database); PostgreSQL 17 (`devpg`, databases `blind_a` and `blind_a_restore`, both dropped at the end). Roles `app` and `app_migrate` already existed in the cluster and were left in place.
- Persona: Django developer, little SQL
- Tester model: Claude Sonnet 5.5 (rustc 1.98.1)
- App: `/tmp/rango-blind-a` (lib crate + `library` bin running every step, bins `errors`, `probe`, `sqlcount`, `retry` (PG only); builds with `--features sqlite` and `--features postgres`).

## (a) Friction log

| # | Step | Severity | What happened | Expected | Exact error | Suggested fix |
|---|---|---|---|---|---|---|
| 1 | 3 | MINOR | With 5 models + 2 pivots, the migration is `0001_auto.sql`. | `NNNN_<tables>.sql` (e.g. `0001_post_user.sql`) as in the quick start. | none | Say when `auto` is used, or name from the tables. |
| 2 | 2 | MINOR | `FieldVarchar::new(..)?` in a fn returning `rango::Result` does not compile; same for `DatabaseConfig::from_env()?` (`VarError`). I switched my helpers to `anyhow::Result`. `FieldPassword::new` / `verify` exist only in rustdoc. | `FieldError` convertible, or the guide shows the pattern. | ``error[E0277]: `?` couldn't convert the error to `rango::Error` ... the trait `From<FieldError>` is not implemented for `rango::Error` `` | Add `From<FieldError>` (and a note in `errors.md`); document `FieldPassword::new/verify` in the guide. |
| 3 | 6 | DOC-WRONG | `field-types.md` says to use `min` / `max` for exact decimals. `Loan::filter(&pool).max::<Decimal>("fee")` does not compile on SQLite. Workaround: `.order_by("-fee").limit(1).one()`. | `max::<Decimal>` works on every backend, or the page says it does not. | ``error[E0277]: the trait bound `for<'r> rango::Decimal: sqlx_core::decode::Decode<'r, sqlx_sqlite::database::Sqlite>` is not satisfied`` (also `Type<Sqlite>`) | Fix the page (SQLite excluded) or implement the decode. |
| 4 | 2 | MINOR | `#[derive(Model)]` gives no `Clone` / `Debug`; the guide never says to add them. | A word in the models page. | ``error[E0599]: no method named `clone` found for struct `Book` `` | Document `#[derive(Model, Clone, Debug)]` (`FieldPassword` Debug is redacted). |
| 5 | 6 | MINOR | `.all()` always returns `Vec<WithRelated<M>>`, even with no relation. Getting the plain `M` needs `.inner`, which only rustdoc shows. | Guide mentions it. | none | One line in `filter.md`. |
| 6 | 5 | PAINFUL | No bulk API for ManyToMany. Seeding 1200 books with 1-2 authors and 2 tags each is about 3600 `M2M::add` statements (seed step = 3994 statements, 4.4 s SQLite, 6.9 s PG). | `bulk_create` for pivots, or a documented recipe. | none | Add `M2M::add_many`, or document that pivots are plain tables. |
| 7 | 7, 10 | MINOR | On SQLite a violation of the partial unique index reports the columns, so "one active loan per book" cannot be told from another unique on `loan.book_id`. PG gives the index name. | Index name on both. | `UniqueViolation { constraint: Some("loan.book_id") }` (PG: `Some("uniq_active_loan")`) | Document the difference (it is documented as "failing columns" but not in the constraints page example). |
| 8 | 5 | MINOR | Hooks do not run in `bulk_create` (documented), so seeded emails stayed `"Member0@Example.COM "`. Surprising for a Django developer, though accurate. | Warning in `hooks.md` next to the seed pattern. | none | Cross-link bulk.md and the hooks page. |
| 9 | 4 | MINOR | Evolve works: add field, rename (pty prompt `Did you rename "member"."name" to "full_name"? [y/N]`), CHECK constraint (SQLite rebuild) all kept the 1503 loans. The non-tty message is excellent: `Column "member"."full_name" was added while another column was dropped: this may be a rename of "name", which needs confirmation. Run `rango makemigrations` in a terminal and answer `y`, or pass `--yes` ...` | OK. | none | none |
| 10 | 1, port | DOC-WRONG | `configuration.md`: `[models] src = "src"` (directory to scan) is ignored. With `src = "mymodels"` (dir exists), `"../src"` or an absolute path the tool prints `Scanning models in src...` and `No models found.`. Only the positional arg works (`rango makemigrations ../src`). Repro: any dir with `rango.toml` `[models] src = "other"` and models in `other/`. | The key is honoured. | `No models found.` | Honour the key, or remove it from the docs. |
| 11 | 8 | PAINFUL | `atomic_retry` inside `tokio::spawn(async move { ... })` with a non-`move` async closure that borrows a captured `Uuid` fails to compile. `transactions.md` says the future is `Send` and spawn "works". Fix: `async move |tx|` with `Copy` values copied first. (I only tried `atomic_retry`, not plain `atomic`.) | Page's example compiles for realistic closures. | ``error: implementation of `Send` is not general enough ... `Send` would have to be implemented for the type `&'0 uuid::Uuid`, for any lifetime `'0`... `` | Show the `async move |tx|` form with owned captures. |
| 12 | 10 | MINOR | String filter with a value of the wrong type: SQLite silently returns 0 rows (`eq("born_year","not-a-number")`, `eq("id","not-a-uuid")`); PG returns a raw driver error. | `Error::Invalid` on both, like unknown columns. | PG: `Database { code: Some("42883"), message: "operator does not exist: integer = text" }` (and `uuid = text`) | Check value type against the schema, as is done for column names. |
| 13 | 10 | MINOR | FK to a missing row on SQLite has no constraint name; PG names it. | Consistent. | SQLite: `ForeignKeyViolation { constraint: None }`; PG: `Some("loan_book_id_fkey")` | Document. |
| 14 | 10 | MINOR | `delete` of an unsaved row returns `Ok(())` silently, `update` of one returns `NotFound`. | Same behaviour. | `Ok(())` | Return `NotFound` or document. |
| 15 | 11 | MINOR | `restore -d postgres://...` run in the project directory whose `rango.toml` says `backend = "sqlite"` ignores the URL scheme and tries SQLite. Same for a gzip backup. Repro: `rango restore dumps/full.json -d postgres://dev:dev@127.0.0.1:5432/x`. | Backend taken from the URL, or a clear message. | `Error: Failed to connect to SQLite ... database error 14: unable to open database file` | Infer from the URL, or say "rango.toml says sqlite but the URL is postgres". Docs on dump.md should tell you to run from a PG-configured directory when moving across backends. |
| 16 | 11 | MINOR | `rango export --table author -o a.json` has no `__table`; `rango import a.json` (no `-t`) fails. The CSV round trip needs `-t`. | Clear message. | `Failed to insert into default ... relation "default" does not exist` | Error "no __table and no --table", or write `__table` in single-table export. |
| 17 | 11 | MINOR | `rango dump --schema-only` prints a misleading count. | `Dumped N table(s)` or "schema". | `✅ Dumped 0 table(s) to dumps/schema.json` | Reword. |
| 18 | 11 | MINOR | After restoring the SQLite dump into PG, `_rango_migrations` holds the SQLite 4 migration names while my PG `migrations/` has 1 file. `rango migrate` says up to date; the two histories silently disagree. | A warning, or doc on cross-backend history. | `Nothing to apply - database is up to date.` | Document: after cross-backend restore, regenerate migrations from PG or keep one migration set. |
| 19 | 11 | MINOR | `rango export` JSON keys are alphabetical, not column order. | Schema order. | none | Cosmetic. |
| 20 | 1, port | MINOR | `credentials.md` `CREATE ROLE app_migrate ...` is not idempotent and roles are cluster-global, so a second project fails. | `CREATE ROLE ... IF NOT EXISTS`-style snippet (`DO $$ ... $$`). | `role "app_migrate" already exists` (the roles pre-existed in the shared cluster) | Add a `DO` block or a note. |
| 21 | 9 | MINOR | Statement counts differ by backend for the same code: `atomic` adds a logged `COMMIT` on PG (borrow_many of 2 books: 3 on PG, 2 on SQLite). Events come from sqlx target `sqlx::query` at DEBUG; no `BEGIN` logged on either. | Same counting rule. | none | `tracing.md`: say which statements are logged. |
| 22 | 5 | MINOR | Debug build: 20 argon2id hashes cost about 4 s of the seed. | Hint. | none | Mention `[profile.dev.package.argon2] opt-level = 3` or similar in the password page. |

## (b) Per step: Rust lines and SQL queries

Rust lines (`wc -l`): `models.rs` 82, `ops.rs` 281, `db.rs` 18, `obs.rs` 49 (tracing counter), `main.rs` 125 (scenario), `bin/errors.rs` 90, `bin/probe.rs` 23, `bin/retry.rs` 38, `bin/sqlcount.rs` 14. Migrations are generated (83 lines of SQL, 4 files).

| Step | Rust lines (approx) | SQL queries SQLite | SQL queries PG |
|---|---|---|---|
| 1 install, init, configure | 18 (`db.rs`) + `rango.toml` 10 | 0 | 0 |
| 2 models | 82 | 0 | 0 |
| 3 makemigrations + migrate | 0 (CLI) | DDL by CLI | DDL by CLI (via `MIGRATE_DATABASE_URL`, `app_migrate`) |
| 4 evolve (add field, rename, CHECK) | model edits only, 3 migrations | CLI; rebuild kept 1503 loans | not re-run; PG got one generated migration from the final models |
| 5 seed (300 authors, 1200 books, 32 tags, 21 members, 1503 loans) | about 60 | 3994 | 3994 |
| 6 queries: late_loans / history_page (4 pages + empty) / books_out / never_borrowed / active_members / search / fee_stats | about 70 | 1 / 5 / 2 / 1 / 1 / 3 / 5 | same |
| 6b CRUD helpers: borrow, get_or_create, update_or_create, bulk_update, bulk_upsert | about 30 | 1 / ... / 1 / 1 | same |
| 7 transaction (`borrow_many` failing, then ok; `return_book`) | about 35 | 2, 2, 2 | 3, 3, 3 |
| 8 concurrency: 8-task race | about 15 | 14 statements; ok=1, already_out=7 | same |
| 8b `atomic_retry`, SERIALIZABLE, 6 tasks, limit 3 active loans | 38 | n/a | 13 attempts for 6 tasks; exactly 3 loans at the end |
| 9 tracing counter | 49 | | |
| 10 deliberate mistakes | 90 | see log | see log |
| 11 dump, restore, backup | 0 (CLI) | | restored 7004 rows into empty PG; sum of fees 1734.50 and 1504 loans identical to SQLite; partial index, CHECK and FKs recreated; argon2 hash preserved |

## (c) What felt simple

- Models are one struct each; the partial unique index, CHECK and default ordering are one attribute line.
- `makemigrations` / `migrate`; the SQLite rebuild is invisible and kept the data. The rename prompt and its non-tty message are the best part.
- Errors: `TableMissing { table: "author" }` when I forgot `migrate`; unknown column errors with a suggestion (`did you mean `name`?`) in filter, order_by, sum, values and bulk_update; `CheckViolation { constraint: Some("chk_fee_positive") }`.
- Porting SQLite to PG: only `connect()` changed (and one cfg for `connect_sqlite`); no model or query code changed. Race and slow-query WARN behaved the same on both.
- `atomic`, `select_related`, `prefetch`, `where_exists`, keyset `.before()`, `get_or_create` and bulk update/upsert did what the docs say; `explain_analyze` gives a full plan on PG and `SEARCH loan USING INDEX loan_member_id_idx (member_id=?)` on SQLite.
- Least privilege: migrating through `MIGRATE_DATABASE_URL` worked; as `app`, `CREATE TABLE` gives `permission denied for schema public` and DML works.
- Dump/restore/backup: `0600` files, gzip auto-detected, restored in one transaction in a fresh PG database.

## (d) Doc pages wrong, missing or misleading

- `field-types.md` (wrong): `max::<Decimal>` advice does not compile on SQLite (#3).
- `configuration.md` (wrong): `[models] src` is ignored (#10).
- `transactions.md` (misleading): `tokio::spawn` + `atomic_retry` needs `async move` and owned captures (#11).
- `errors.md` (missing): `FieldError` is not a `rango::Error` (#2); wrong-type string filter behaves differently per backend (#12).
- Models page (missing): `Clone` / `Debug` derives (#4); `WithRelated.inner` (#5).
- `bulk.md` (missing): nothing for many-to-many seeding (#6).
- `cli/export.md` / `cli/import.md` (misleading): single-table export cannot be re-imported without `-t` (#16).
- `cli/dump.md` (missing): backend comes from `rango.toml`, not from `-d` (#15); migration history after cross-backend restore (#18).
- `security/credentials.md` (misleading): role creation is not repeatable (#20).
- `tracing.md` (missing): which statements are logged (`COMMIT` on PG only) (#21).
- Quick start (misleading): migration file name (#1).

## (e) Verdict

Score: 7/10.

I would use Rango for this app. Models, schema evolution and relations were easier than I expected from Django, and nothing blocked me. What would keep me from 9: no bulk many-to-many (3600 statements to seed), a doc claim about decimals that does not compile, the ignored `[models] src` key, and rough edges when moving data between backends (#15, #16, #18). Wrong-type string filters should be an `Invalid` error like unknown columns.
