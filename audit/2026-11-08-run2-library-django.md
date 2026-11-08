<!-- Header added by the maintainer; the report below is the tester's text, unchanged. -->

| | |
|---|---|
| Date | 2026-11-08 |
| Rango commit | about 9860de9 (right after audit lots 1-4 were merged) |
| Backends | SQLite, and a PostgreSQL port of the same code |
| Persona | Django developer, little SQL |
| Tester | An AI agent (Claude, Sonnet-class) reading the docs only |
| Note | Run before the current protocol existed: it covers campaign A only, and the steps are numbered differently. |

# Rango blind test 2 (Django dev, not a SQL expert)

App: /tmp/rango-dogfood2/library (SQLite, `cargo run` runs steps 5-9 with query counts), models in src/models.rs, error experiments in examples/err_*.rs. Postgres port: /tmp/rango-dogfood2/library-pg (same code, 2 lines changed; DB dropped).
Deviations (not friction): path dependency, CLI built from the repo and run from target/debug/rango. Crate version is 0.2.0-beta.1 while docs say `0.1`.
Source peeks: none. I only saw repo paths inside compiler error output. No BLOCKER needed source.

## (a) Friction log

| # | Step | Sev | What happened | Expected | Exact error | Suggested fix |
|---|---|---|---|---|---|---|
| 1 | 12 | PAINFUL | `rango restore` into a fresh DB creates tables and rows but not `_rango_migrations`. A later `rango migrate` replays 0001, then 0002 dies | restore marks migrations as applied (or docs say "run migrate first, then restore --data-only") | `Failed to apply 0002_book.sql ... duplicate column name: pages` | Record applied migrations on restore, or document the restore-then-migrate order |
| 2 | 6 | PAINFUL | Filtering a Decimal column with a plain number is a compile error and the message does not say what to pass. Docs never show how to build a `Decimal` (`rango::Decimal::new(2000, 2)`) | `.lt(Book::PRICE, 20)` | ``Column<Book, FieldDecimal<10, 2>>` is not a column of this model that accepts a `{integer}` value` ... `the trait FilterColumn<Book, {integer}> is not implemented` | Add a Decimal section to field-types.md / filter.md (construct, filter, sum). Add a custom `on_unimplemented` hint naming the expected value type |
| 3 | 6 | PAINFUL | `sum`/`avg` on a money column return `Option<f64>` (SQL is `CAST(SUM(price) AS REAL)`). Prints 46.97, but it is float money | a `Decimal` back | none | Return `Option<Decimal>` for Decimal columns, or warn in aggregations.md |
| 4 | 2/5 | PAINFUL | Building a value is long: 5 models need a helper each (35 lines). Uuid, date and datetime each need a manual conversion (`NaiveDate::from_ymd_opt(..).into()`, `Decimal::new(..)`, `ManyToMany::default()`, `None` for each optional) | `Book::new(title=..., ...)` or `..Default` | none | A `#[derive(New)]`/builder, or `Default` for ids and timestamps. The docs admit this on purpose (defining-models.md) |
| 5 | 3-4 | MINOR | `connect_sqlite(&str)` but `connect_postgres(&DatabaseConfig)`. I copied the SQLite call and got a type error | same signature | `expected &DatabaseConfig, found &String` | Accept a URL on both, or put a PG example in quick-start |
| 6 | 4 | MINOR | Rename: `makemigrations` fails with a good message, but renames are not documented in migrations/makemigrations.md. `--yes` appears only on cli/makemigrations.md as "confirm renames and drops". The answer `y` could not be piped without a TTY | a paragraph "Renaming a field" | `Column "member"."full_name" was added while another column was dropped: this may be a rename of "name", which needs confirmation. Run ... pass --yes ...` | Document renames and what `--yes` does (it picked RENAME COLUMN, good) |
| 7 | 3 | MINOR | Migration file names are not descriptive: `0002_book.sql`, `0003_member.sql`, `0001_author_book_book_tag_loan_member_tag.sql` | Django-like `0002_book_pages` | none | Name by change type (`add_book_pages`) |
| 8 | 6 | MINOR | `exists()` runs `SELECT COUNT(*) ... WHERE ...` (no short-circuit), so on a big table the DB counts every match | `SELECT 1 ... LIMIT 1` | none | Use `EXISTS`/`LIMIT 1` |
| 9 | 9 | MINOR | SQLite bulk_create of 1,000 rows is 9 statements and bulk_update is 3 (999-parameter chunking). The bundled SQLite 3.46 allows 32,766 parameters | 1 statement | none | Raise the SQLite chunk size |
| 10 | 11 | MINOR | A string column with a wrongly typed value is silent on SQLite: `.lt("price", "abc")` returns all 1,000 rows (SQLite compares text > number) | error | `Ok(1000)` | Docs: say that string names are unchecked and prefer typed columns. Maybe validate string-name values against the schema |
| 11 | 5 | MINOR | `insert` of a `Member` takes ~200 ms in a debug build (argon2id, one per row) | n/a | none | One line in field-types.md: `FieldPassword` hashes on write, slow in debug |
| 12 | 2 | MINOR | `rango.toml`/quick-start say `rango-orm = "0.1"`, but the repo crate is 0.2.0-beta.1 | | | Update version in installation.md |
| 13 | 6 | MINOR | `where_exists` for "has an active loan" must name the FK column on the child (`Loan::BOOK_ID`), not the parent. Fine once read, but unusual for a Django user | | none | Add an example with the parent/child named |
| D1 | 2 | DOC-WRONG | models/defining-models.md says "`ManyToMany` still needs UUID primary keys on both sides, and `ForeignKey<M>` points at UUID keys only", but relations.md says integer keys work | | | Delete the stale sentence |
| D2 | 2 | DOC-WRONG | models/field-types.md lists `ForeignKey<M>` as "stored as UUID", but relations.md says the column follows the target key | | | Fix the table |
| D3 | 2 | DOC-WRONG | field-types.md: `FieldPassword<MIN,MAX>` = `VARCHAR(MAX)` on Postgres. Generated SQL is `"password" TEXT NOT NULL` (right, a hash is longer than MAX) | | | Fix the table |
| D4 | 3 | DOC-WRONG | getting-started/quick-start.md and migrations/makemigrations.md show names like `0001_post_user.sql`/`0001_post_tag_user.sql`, and the model counts and sample output differ (docs also never show the "M2M relation" lines) | | | Refresh the sample output |
| D5 | 6 | DOC-WRONG | querying/filter.md `.explain()` sample uses `"myapp_user"` although docs say tables are never prefixed | | | Fix the sample |
| D6 | 12 | DOC-WRONG | cli/export.md and import.md use `myapp_user` / `myapp_post` table names, which no longer exist since prefixes were removed (configuration.md says so) | | | Fix the samples |


## (b) Per-step lines of Rust and SQL query count (SQLite; Postgres identical except where noted)

| Step | Rust lines (mine) | SQL statements | vs hand-written |
|---|---|---|---|
| 1 init/config | 0 (Cargo.toml: 6 deps, 1 `rango init`) | 0 | n/a |
| 2 models + hook + constraint | 62 models.rs (hook 9, constraint 3) + 35 lines of constructor helpers | 0 | n/a |
| 3 makemigrations/migrate | 0 | 0 | n/a |
| 4 schema change | 3 lines edited, 3 makemigrations, 3 migrate | 0 | SQLite rebuilt `loan` (create new, copy, drop, rename) for the unique constraint: correct |
| 5 CRUD | 24 | 13 (7 inserts, get, update, insert, insert, delete, get) | equal. insert uses RETURNING, update has no pre-SELECT, delete cascade is one DELETE |
| 6 queries | 32 | 13 | equal |
| 7a select_related (book and member) | 8 | 2 (1 insert + 1 JOIN) | equal, no N+1 |
| 7b M2M + prefetch | 23 | 11 (2 tag inserts, 3 add, 1 remove, 1 all, 2 prefetch = 2 queries each) | equal |
| 8 transactions (borrow, fail, rollback) | 32 | 6 on SQLite (BEGIN/COMMIT not logged), 9 on Postgres (they are) | equal |
| 9a bulk_create 1000 | 6 | 9 SQLite (chunks), 2 Postgres (1 UNNEST + tx control) | SQLite worse than hand-written (1-2 statements possible) |
| 9b bulk_update 1000 | 6 | 3 SQLite, 2 Postgres | same remark |
| 10 tracing counter | 22 (a `Layer` counting `sqlx::query` events) | n/a | n/a |
| 11 errors | ~40 in examples/ | n/a | n/a |
| 12 dump/restore | 0 | n/a | n/a |

No N+1 and no re-fetches found. Flagged: `exists()` is a COUNT (F8), SQLite bulk chunking (F9).
tracing.md worked, but you must write your own counting `Layer`: there is no ready-made "count queries" helper. Counts for SQLite do not include BEGIN/COMMIT (Postgres does), so numbers differ by backend.

## Task 11: four mistakes, would a beginner understand?

1. Wrong column, typed (`Book::TITEL`): `error[E0599]: no associated function or constant named TITEL found for struct Book` with `help: there is an associated constant TITLE with a similar name`. Excellent.
   Wrong column, string (`"titel"`): runtime `Invalid("unknown column \`titel\` on table \`book\`, did you mean \`title\`?")`. Excellent.
2. Wrong value type (`.lt(Book::PRICE, "cheap")`): `` `Column<Book, FieldDecimal<10, 2>>` is not a column of this model that accepts a `&str` value `` plus `the trait FilterColumn<Book, &str> is not implemented ...`, note "pass a typed column such as `Model::NAME` with a value of its field type". A beginner understands the first line, but not what to pass (Decimal). With a string column name it is silent (F10).
3. Forgot migrate: `TableMissing { table: "author" }`. Clear, and the docs say what to run. Nit: the Display message is not "run rango migrate".
4. Unique violation: `UniqueViolation { constraint: Some("author.email") }`, Display `unique violation (author.email)`. Clear. The hook lowercased the email, so `DUP@x.com` collided with `dup@x.com`, as intended.

## (c) What felt genuinely simple

- Models to working SQL in minutes: derive, `makemigrations`, `migrate`. SQL was readable, FKs indexed, `ON DELETE CASCADE` real (verified: deleting an author removed its books in 1 DELETE).
- Same code ran on SQLite and Postgres with 2 lines changed; Postgres got `UUID`/`TIMESTAMPTZ`/`DATE`, SQLite `TEXT`.
- Typed columns: typos caught by the compiler with a suggestion.
- Relations: `select_related` x2 = 1 query, `prefetch_related` = 2 queries, M2M `add` idempotent. No N+1 anywhere.
- Hook: 9 lines, worked first time. `atomic` with `Error::abort` + rollback worked first time.
- Changing the schema (add column, rename with `--yes`, composite unique): all correct, including the SQLite table rebuild.
- Error variants (`TableMissing`, `UniqueViolation`) are human-friendly.
- Genuinely less simple: building values (F4), Decimal and date handling (F2, F3), `rango::Decimal` / `chrono` / `uuid` crates that you must add yourself, async closure transaction syntax (`&mut **tx`, `pool.clone()` captured in a closure), restore (F1).

## (d) Doc pages wrong, missing or misleading

- models/defining-models.md: stale "UUID keys only" sentence (D1); no Decimal/date construction examples.
- models/field-types.md: `ForeignKey` stored as UUID (D2), `FieldPassword` type (D3), no Decimal usage.
- models/relations.md and querying/relations.md: which child FK `where_exists` needs.
- migrations/makemigrations.md: no rename section, sample output outdated (D4), "What gets detected" table has no rename / constraint / index row.
- getting-started/quick-start.md: version "0.1" vs 0.2.0-beta.1, sample file names.
- getting-started/configuration.md and backends/postgres.md: `connect_postgres` needs `DatabaseConfig` (shown only in a pool-tuning section, not the Quick start).
- querying/filter.md: `myapp_user` in `.explain()` sample (D5); no Decimal example.
- querying/aggregations.md: does not warn that `sum`/`avg` are `f64` for Decimal columns.
- cli/export.md, cli/import.md: `myapp_*` names (D6).
- cli/dump.md: says nothing about migrations after a restore (F1).
- operations/tracing.md: no recipe to count queries per request.

## (e) Verdict

Score: 8/10. For everything I did, I never wrote or read SQL, and the migration SQL was right every time, including SQLite table rebuilds, cascades and a rename. The query layer did what a hand-written version would (no N+1, no re-fetch). The compiler errors for typed columns guide a beginner well. Points off: money handling (Decimal construction, `f64` sums), verbose value construction, the restore-then-migrate trap, and a few stale doc snippets. "You don't need to be a SQL expert" holds for day-to-day use; you still need to understand the SQL concepts behind unique constraints, cascade and N+1 to choose the right call (`where_exists` vs `select_related`).

## (f) Decisions

Checked against the tree at commit cd9a7cd (2026-11-08).

| # | Decision | Commit or reason |
|---|---|---|
| 1 | fixed | eac1425: restore records the applied migrations; documented in cli/dump.md. |
| 2 | documented | 5fb5fd5: field-types.md has a "Working with decimals" section (construct, filter, sum). The compiler message still does not name the value type to pass (c8c74bc added `on_unimplemented` notes). |
| 3 | documented | 5fb5fd5: aggregations.md and field-types.md warn that `sum`/`avg` return `f64`. Returning `Decimal` not done. |
| 4 | won't fix | By design: defining-models.md says "There is no generated constructor: the fields are the API", and the compiler lists missing fields. `.into()` is the short form. |
| 5 | documented | a12b529: quick start shows `connect_postgres(&DatabaseConfig::from_url(..))` and `connect_mysql(url)` next to the SQLite call. API unchanged. |
| 6 | fixed | 1c25338 and 5fb5fd5: renames and `--yes` documented in migrations/makemigrations.md; the non-TTY error names the rename. |
| 7 | won't fix | Names come from the tables touched (`NNNN_<tables>.sql`, documented in makemigrations.md). Descriptive change names are not planned; the SQL is the readable part. |
| 8 | fixed | 7fbd6ba: `exists()` runs `SELECT EXISTS(SELECT 1 ...)`. |
| 9 | fixed | 7b0c1c9: SQLite bulk operations chunk at 32,000 bound parameters (bundled SQLite 3.46 allows 32,766), so 1,000 rows are one statement. |
| 10 | documented | filter.md says string names "accept any value and are checked by the database at runtime". No value validation added. |
| 11 | documented | 89d82f0: field-types.md notes the argon2id cost per write and that bulk inserts pay it per row. |
| 12 | fixed | Version is `0.2` in README, installation.md, postgres.md and the crate rustdoc (5fb5fd5). |
| 13 | documented | filter.md Subqueries explains the FK column points at the parent and shows `Article::AUTHOR_ID` (84a39ad). No extra example added. |
| D1 | fixed | 5fb5fd5: stale "UUID keys only" sentence replaced; defining-models.md lists the integer key types. |
| D2 | fixed | 5fb5fd5: field-types.md says the FK column has the type of the target key. |
| D3 | fixed | 5fb5fd5: `FieldPassword` row says `TEXT` (argon2id hash). |
| D4 | documented | 5fb5fd5: sample output refreshed and the naming rule stated. The "M2M relation(s)" lines the CLI prints are still not shown in the samples. |
| D5 | fixed | fd2d824: no `myapp_` prefix left in filter.md. |
| D6 | fixed | fd2d824: no `myapp_` prefix left in cli/export.md and import.md. |
