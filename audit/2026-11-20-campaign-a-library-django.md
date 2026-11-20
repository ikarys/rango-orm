---
date: 2026-11-20
commit: 7e0d1ec
kind: blind-campaign-a-library-django
tool: rango-orm 0.2.0 (path dependency) + rango CLI built from the repo
model: Sonnet 5.5 (claude-sonnet-5-5)
scope: Library app (books, authors, tags, members, loans), SQLite then PostgreSQL 17; persona Django developer, little SQL
verdict: 8/10, would use it
---

## Brief

Persona: Django developer, little SQL. App: library with books (several authors and tags), members with passwords, loans with one active loan per book (partial unique index), late loans, paginated history. Databases: SQLite file (bundled engine), then PostgreSQL 17 (`blind_a`, roles `blind_a_migrate` / `blind_a_app`, all dropped at the end). Seed: 1,000 authors, 50 tags, 5,000 books, 2,000 members, 100,000 loans.

Blindness: I read only `docs/src/**` and `README.md` of the repo. I never opened source, tests, examples, AGENTS.md, CHANGELOG, audit/ or git history. Rust compiler errors printed repo paths (`rango-orm-core/src/filter.rs`, ...); I did not follow them. No `BLOCKER: needed source` was needed. App in `/tmp/rango-blind-a` (lib + 6 bins: seed, queries, scenarios, mistakes, retry, extras), builds with `cargo build --release` (SQLite) and `--no-default-features --features postgres` (PG). PG migrations live in `pg/`.

## (a) Friction log

| # | Step | Severity | What happened | Expected | Exact error | Suggested fix |
|---|---|---|---|---|---|---|
| 1 | 12 (port) | PAINFUL | `rango restore` of a SQLite dump into PostgreSQL records the SQLite migration names (`0001_auto.sql` .. `0005_loan.sql`) in `_rango_migrations`. The PG project (own `pg/` folder, own numbering `0001_auto.sql`, `0002_loan.sql`) then replays `0002_loan.sql` on a database that already has the column. Repro: `DATABASE_URL=sqlite://db.sqlite3 rango dump -o d.json`; `DATABASE_URL=postgres://dev:dev@127.0.0.1:5432/blind_a_restore rango restore d.json`; `cd pg && DATABASE_URL=<same> rango migrate`. | Docs on restore across backends say nothing about the migration set of the target; I expected a warning or a documented procedure. | `Error: Failed to apply 0002_loan.sql` / `error returned from database: column "renewals" of relation "loan" already exists at line 7487` | In dump.md (cross-backend) and sqlite.md: say the target backend needs its migration folder generated first and that restore records the source names; suggest `rango migrate` on the empty target, then `restore --data-only`-style import, or have restore warn when the target folder names differ. |
| 2 | 12 (port) | MINOR | The "at line 7487" in the error above points to no line of the 1-line migration file. | A line within the file or none. | `... already exists at line 7487` | Drop or correct the position. |
| 3 | 5 | PAINFUL | `M2M::add_many` links ONE row to a list. Tagging 5,000 books with 2-3 tags needs 5,000 calls (SQLite 1.7 s and 2.0 s for authors/tags, PostgreSQL 11.8 s and 12.9 s, 10,000 statements each). No API to insert many (left, right) pairs at once. The brief's "tag books in batch" only works per book. | A bulk pair insert, or a doc note to use `raw_execute` for seeding pivots. | (none, slow) | relations.md: add a "seeding many links" paragraph (raw `INSERT` into `book_tag`) or add `M2M::add_pairs`. |
| 4 | 11 | PAINFUL | A string column name with a value of the wrong type behaves differently per backend. SQLite silently returns no rows, PostgreSQL returns a raw DB error. Repro: `Loan::filter(&pool).eq("late", "banana").count()`. Typed columns catch it at compile time (clear error), but the docs push strings in many examples. | Same behaviour on both; `Error::Invalid` like for unknown names. | SQLite: `Ok(0)`. PG: `Database { code: Some("42883"), message: "operator does not exist: boolean = text" }` | filter.md says strings "are checked by the database at runtime": add that SQLite does not check, so a port to PG can start failing; recommend typed columns. |
| 5 | 4/7 | MINOR | `.explain()` returns only the SQL with placeholders, not the plan. The brief and a Django habit (`QuerySet.explain()`) expect an index check from it; only `.explain_analyze()` shows the index (and it runs the query). | `.explain()` showing the plan (as in Django). | `SELECT * FROM "loan" WHERE "member_id" = ? ORDER BY "id" DESC LIMIT 20` | filter.md: say clearly at the top of "Inspecting the SQL" that index checks need `explain_analyze()`; consider `explain_plan()` that does not execute on PG (`EXPLAIN` without ANALYZE). |
| 6 | 12 | DOC-WRONG | credentials.md says "`rango dump` and `rango backup` must read every table, including `_rango_migrations`. The DML-only app role is not enough". With the grants given on the same page (`ALTER DEFAULT PRIVILEGES ... GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO app`), the app role dumped all 7 tables fine (36 MB), because the migration role creates `_rango_migrations` and the default privileges apply to it. The flip side is not documented: the app role can also `DELETE FROM _rango_migrations`. | The page matches the behaviour, or tells how to keep the bookkeeping table away from the app role. | `✅ Dumped 7 table(s) to ...` (rc 0, app role) | Reword: "with these grants the app role can read and write `_rango_migrations` too"; add a `REVOKE ... ON _rango_migrations FROM app` line if that is not wanted. |
| 7 | 1 | DOC-WRONG | README "Features" table: `Keyset pagination, optimistic locking | planned`, while keyset pagination (`.after()` / `.before()`) is implemented and documented in pagination.md; I used it on both backends. | Row says keyset is done. | (n/a) | Split the row: keyset done, optimistic locking planned. |
| 8 | 9 | MINOR | `atomic_retry` does not exist for SQLite (documented in transactions.md, but only in the last sentence of a long paragraph). Calling it on a SQLite build gives a bare compile error with no hint. | A hint, or the doc statement near the signature. | `error[E0425]: cannot find function `atomic_retry` in this scope` | Put "Postgres and MySQL only" at the start of the "Retrying on conflict" section. |
| 9 | 5/7 | MINOR | `bulk_create` skips hooks (documented); my `Book` slug hook did not run and `slug = "x"` was stored. A Django developer expects `save()` signals. The doc says it three times, but the failure is silent. | A debug-build warning, or an `Error` when a model with `#[model(hooks)]` goes to a bulk op. | (none) | Optional: a `tracing` WARN when a bulk op is called on a model that has custom hooks. |
| 10 | 12 | MINOR | `rango restore` prints a long warning ("these migrations were recorded as applied without running them ... custom SQL ... NOT reproduced") even when every migration is generated and has no custom SQL. A newcomer reads it as a problem. | A warning only when a migration holds SQL the snapshot cannot rebuild, or one neutral line. | `warning: the schema was rebuilt from the dump's snapshot and these migrations were recorded as applied without running them: 0001_auto.sql, ...` | Soften, or skip the warning when files carry no hand-written statements. |
| 11 | 12 | PAINFUL | `rango restore` is slow on PostgreSQL: 104,262 rows (36 MB JSON) in 48.7 s, versus 7.5 to 8.2 s into SQLite and 0.8 s for `bulk_create` of 100k rows. | Same order as the dump (0.6 s). | (none) | Document the expected rate (about 2,100 rows/s into PG); use multi-row insert / UNNEST like `bulk_create`. |
| 12 | 4 | MINOR | `makemigrations` writes the rename question as an error when run without a terminal; the message is good (mentions `--yes`), but it is an error with exit code 1 even for a plain `--dry-run`. | OK as documented; noting the first reaction is "it broke". | `Error: Column "author"."full_name" was added while another column was dropped: this may be a rename of "name", which needs confirmation. ...` | Fine; maybe start the message with "Possible rename". |

Not logged as findings (checked, fine): unknown-column error with "did you mean", typed-column compile errors, `TableMissing` on a forgotten migrate, partial unique index after SQLite table rebuild, constraint names on PG.

## (b) Effort per step

Statement counts come from a `tracing` layer counting events on target `sqlx::query` (BEGIN/COMMIT count). On SQLite the first statement of a run includes one connection setup statement, and several figures include statements of the previous action (shown as ~).

| Step | Lines of code written | Calls or queries made |
|---|---|---|
| 1 install, init, config | 10 (Cargo.toml) | `cargo install --path`, `rango init`; 1 doc dead end: how to build the app for 2 backends (none documented) |
| 2 models | 86 (models.rs: 5 models, 2 M2M, 2 hooks, ordering, check + partial unique) | 0 |
| 3 makemigrations + migrate | 0 | 1 + 1 (0001, 12 tables-ish in one file) |
| 4 evolve (rename, add column, add check) | 3 edited lines | 2 makemigrations (`--yes`), 1 migrate; SQLite rebuild of `loan` automatic |
| 5 seed | 57 (seed.rs) | SQLite: 22 stmts for 100k loans, 114,400 rows/s; PG: 1 UNNEST stmt, 118,500 rows/s. M2M: 10,000 statements (see #3). Full seed 4.6 s SQLite, 25.7 s PG |
| 6 queries | 76 (queries.rs) | see step 7 |
| 7 volume | 0 (same bin) | statements per query: overdue + 2 `select_related` = 1; prefetch authors+tags on 100 books = 3; every filter/aggregate/subquery/group_by = 1; keyset page = 1; offset page = 1. No N+1. |
| 8 transaction | ~25 (scenarios.rs) | borrow two books, second taken: 5 to 6 stmts, rolled back (`UniqueViolation`), count unchanged |
| 9 concurrency/retry | 53 (retry.rs, PG) + 8 tasks borrow same book in scenarios | 8 concurrent borrows: 1 ok, 7 `UniqueViolation`; SERIALIZABLE limit-of-3 rule: without retry 9 of 10 `Conflict`; `atomic_retry(.., 10, ..)`: 34 attempts, exactly 3 loans |
| 10 observability | 24 (obs.rs, a `tracing_subscriber::Layer`) | counts above; slow query WARN seen on both backends |
| 11 deliberate mistakes | 47 (mistakes.rs) | 19 cases |
| 12 dump/restore/backup | 0 | see timings below |

Volume numbers (100,000 loans; PG = PostgreSQL 17):

| Query | SQLite | PG | Index seen with `explain_analyze()` |
|---|---|---|---|
| count loans | 1.9 ms | 4.2 ms | n/a |
| overdue active (300 rows) + 2 select_related | 1.5 ms | 2.4 ms | PG: `uniq_active_loan` partial index (nice); SQLite: `SCAN loan` (no index on `due_at` or `returned_at`; my choice, not a Rango bug) |
| group_by member having | 78.6 ms | 11.4 ms | n/a |
| where_exists (books out) | 0.6 ms | 1.6 ms | `uniq_active_loan` used on both |
| member history page (keyset) | 0.7 ms | 1.1 ms | `loan_member_id_idx` used on both |
| offset page 500 (100/page, 50k deep) | 2.8 ms | 2.9 ms | n/a |
| keyset page 500 | 0.7 ms | 1.4 ms | same rows as the offset page: yes |

At this size keyset is faster by 2 to 4x on SQLite, equal on PG (the table fits in cache). Add-column migration on the 100k table: SQLite `ADD COLUMN` 65 ms; SQLite table rebuild (new check constraint) 202 ms; PG `ADD COLUMN ... NOT NULL DEFAULT 0` 22 ms. QuerySet update (300 overdue rows marked late): 1 statement. QuerySet purge of 28,801 old returned loans: 193 ms SQLite, 33 ms PG, 1 statement each.

Dump / backup (full volume, about 104,260 rows): `dump` SQLite 33 MB in 0.65 s; `backup` 5.0 MB gz in 0.9 s; PG dump 36 MB in 0.56 s, backup 5.07 MB in 0.85 s. Restore of the SQLite dump: into SQLite 7.5 s, into PG 48.7 s; restore of the PG dump into SQLite 8.2 s. `export`/`import` JSON and CSV SQLite<-PG worked (1,000 authors, 51 tags, 5,001 books).

Port SQLite to PG, what changed: Cargo features (`sqlite` vs `postgres`), a 6-line `connect()` (`connect_sqlite(url)` vs `connect_postgres(&DatabaseConfig::from_url(url))`) and a `Pool` type alias. Zero changes in models and queries. A separate `pg/` folder with its own `rango.toml` and regenerated migrations (the SQL differs: `UUID`, `TIMESTAMPTZ`, `BIGINT GENERATED BY DEFAULT AS IDENTITY`). Error details differ: unique violation names `member.email` on SQLite, `member_email_key` on PG; FK violation has no constraint name on SQLite (`ForeignKeyViolation { constraint: None }`) and `loan_book_id_fkey` on PG; string-name wrong type (#4).

## (c) What was simple

- Models to working schema in three commands; first `cargo build` of 480 lines of app code compiled almost without error.
- Rename detection with a useful error in CI mode; SQLite rebuild is automatic and kept the partial unique index and checks.
- Typed columns: the compile error for a wrong column or value type is readable.
- Runtime errors are typed and good: `TableMissing`, `UniqueViolation`, `CheckViolation { constraint: Some("chk_fee_positive") }`, "unknown column `emial` ..., did you mean `email`?".
- Hooks (slug, normalized email) and `FieldPassword` (`verify`, redacted `Debug`) worked first try; `from_hash` made a 2,000-member seed instant.
- `atomic`, `Error::abort`, `atomic_retry` and the partial unique index gave a correct one-active-loan rule under 8 concurrent tasks on both backends.
- Counting statements: 1 per query, 3 for two prefetches; `select_related` with two FKs is one statement.
- Dump/restore/backup need no external tool, and the SQLite dump restored into PG with identity sequence, partial index and checks intact.
- Least-privilege roles: the documented SQL worked as written; `MIGRATE_DATABASE_URL` picked up first.

## (d) Documentation pages wrong, missing or misleading

- `README.md`: Features table says keyset pagination is planned (#7).
- `security/credentials.md`: the claim that the DML-only role cannot dump is false with the grants shown; missing the note that the app role can edit `_rango_migrations` (#6).
- `cli/dump.md` (cross-backend restore) and `backends/sqlite.md`: nothing on how the target's migration folder interacts with the migration names recorded by restore (#1).
- `querying/filter.md`: string names "checked by the database" hides the SQLite/PG difference (#4); `.explain()` does not give a plan (#5).
- `models/relations.md`: no way/advice to link many rows in bulk (#3).
- `getting-started/installation.md`: explains several backends at once but not the common case "same crate, SQLite for dev and PG for prod" (cfg features and different `connect_*`); I had to invent the `Pool` alias.
- `operations/transactions.md`: `atomic_retry` availability buried (#8).

## (e) Verdict

Score: 8/10. I would use Rango for this app: the model-first flow, generated migrations and the one-statement relation loading did what a Django developer expects, and the errors are better than most ORMs. Points lost for: no bulk pair insert for M2M, restore across backends interacting badly with a regenerated migration set (#1), restore speed on PG (#11), and a few doc statements that no longer match the behaviour (#6, #7). No BLOCKER; the 2 DOC-WRONG are one-line fixes.

Extra one-liners (not in the table): `bulk_upsert` returned 0 when the only non-key column is the conflict column (nothing to update), which is correct but the docs say "one per inserted or updated row"; `.related::<Book>()` panic behaviour not exercised; slow query log line begins with two newlines inside `db.statement`.

## (f) Decisions

Checked against the tree at commit 86a9183 (2026-11-20).

| # | Decision | Commit or reason |
|---|---|---|
| 1 | fixed | 31f7acb: `rango restore --migrations <dir>`; across backends it records the target folder's files when its snapshot matches the dump, otherwise nothing, with a warning. Documented in cli/dump.md. |
| 2 | fixed | a7b4249: `migrate` reports the line in the migration file from the Postgres error position, and drops sqlx's "at line N" (a line of PostgreSQL's C source). |
| 3 | documented | be190cb: relations.md shows how to seed many pivot rows with `raw_execute`. A many-to-many pair insert API stays open. |
| 4 | open | Values passed with a string column name are not checked against the column type. be190cb documents the per-backend behaviour in filter.md and points to typed columns. |
| 5 | documented | be190cb: filter.md says `.explain()` is SQL only and index checks need `.explain_analyze()`. A non-executing plan method is not planned. |
| 6 | fixed | be190cb: credentials.md matches the behaviour (the app role can dump), adds the `REVOKE` on `_rango_migrations`, and says the app role cannot run `migrate`. |
| 7 | fixed | be190cb: README row split, keyset pagination done. |
| 8 | fixed | be190cb: transactions.md opens the section with "Postgres and MySQL / MariaDB only". |
| 9 | won't fix | Bulk operations skip hooks by design (as Django's `bulk_create`), documented on three pages. A WARN on every bulk call of a model with hooks would fire on legitimate use. |
| 10 | documented | be190cb: migrate.md explains custom SQL migrations and that restore does not replay them. Telling generated files from hand-written ones, to warn only for the latter, stays open. |
| 11 | fixed | afca4bf: restore and import use multi-row statements (up to 1000 rows). SQLite, 100k rows: 4.46 s before, 0.52 s after. |
| 12 | fixed | e8e5aff: `makemigrations --dry-run` never prompts; it assumes every possible rename and prints a note. |
