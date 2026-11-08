<!-- Header added by the maintainer; the report below is the tester's text, unchanged. -->

| | |
|---|---|
| Date | 2026-11-08 |
| Rango commit | 4d109f7 (before the audit fixes) |
| Backends | SQLite and PostgreSQL (same models and steps) |
| Persona | Django developer, little SQL |
| Tester | An AI agent (Claude, Sonnet-class) reading the docs only |
| Note | Run before the current protocol existed: it covers campaign A only, and the steps are numbered differently. |

# Rango 0.2.0-beta.1 blind usability test (Django developer, no SQL)

App: /tmp/rango-dogfood (SQLite, `just dev` / `cargo run`) and /tmp/rango-dogfood/pg (same models and steps on PostgreSQL).
Rules respected: only the mdBook, README, `cargo doc` output and compiler errors were read. NO source was opened on purpose.
The compiler errors themselves print repo paths (e.g. `rango-orm-core/src/fields.rs:496`); I did not follow them.
Deviations (not friction): path dependency, CLI built from the repo.
No "BLOCKER: needed source" occurred.

## (a) Friction log

| # | Step | Severity | What happened | Expected | Exact error text | Suggested fix |
|---|---|---|---|---|---|---|
| 1 | 5 / 9 | PAINFUL | On SQLite `bulk_create` is NOT atomic: 300 books with a duplicate ISBN in the last chunk left 248 rows inserted (2 chunks of 124 committed). On Postgres (single UNNEST) it is all-or-nothing. | All-or-nothing, or an explicit doc warning. | `UniqueViolation { constraint: None }`, then `rows kept = 248` | Wrap the chunks in one transaction internally; at least document it in bulk.md. |
| 2 | 8 | PAINFUL | Inside `atomic()` the query builder cannot be used: `Book::filter(&pool)` / `QueryBuilder::new` take only a pool. M2M ops and `bulk_create` also take only `&Pool` on SQLite. So "check then insert" in a transaction forces `raw_scalar` with hand-written SQL (`?` vs `$1`, UUID as TEXT on SQLite) - exactly what the user is told they will not write. | `Loan::filter(&mut **tx).eq(..).exists()` | n/a (API gap) | Let QueryBuilder/M2M/bulk accept an executor, or document the limitation in transactions.md. A partial unique index ("one active loan per book") would be the SQL-free answer: document it. |
| 3 | 8 / 5 | DOC-WRONG | bulk.md "Inside transactions" shows `bulk_create(&mut **tx, &users)`. Does not compile on SQLite. | works as documented | `expected &Pool<Sqlite>, found &mut SqliteConnection` | Fix the doc or the API. |
| 4 | all | DOC-WRONG | `use rango::prelude::*; connect_sqlite(..)` (quick-start.md, sqlite.md) does not resolve. It is `rango::connect_sqlite`, not in the prelude. | as documented | `cannot find function connect_sqlite in this scope` | Add to prelude or write `rango::connect_sqlite` in the docs. |
| 5 | 2 | PAINFUL | Building a model with a `ManyToMany` field: no doc says how. `ManyToMany::default()` is the obvious guess and fails with a confusing bound; the answer (`ManyToMany::empty()`) is only in rustdoc. | a documented constructor | `the trait bound models::Tag: Default is not satisfied` (about `ManyToMany::default()`) | Show `ManyToMany::empty()` in relations.md; drop the `M: Default` bound. |
| 6 | 2 | MINOR | The quick-start and CRUD examples use `uuid::Uuid::new_v4()` and `chrono::Utc::now()`, but neither crate is in the "Add to Cargo.toml" list; `rango` does not re-export them. Beginner must discover `cargo add uuid chrono`. `FieldUuid::new()` / `FieldDateTime::now()` exist but the docs use the long form. | deps listed, shorter constructors shown | n/a | Add uuid/chrono to the quick-start Cargo.toml, or re-export; show `FieldUuid::new()`. |
| 7 | 2/5 | MINOR | Docs build values as `FieldVarchar("x".into())`, which skips the validation that `FieldVarchar::new(..)` (returns Result) does. A beginner copying the docs gets unvalidated models. Verbose either way: a 4-field model value is 7-9 lines; I wrote four 6-10 line builder functions (37 lines). | a `Model::new(..)` or builder | n/a | Say which one to use; consider a derived constructor / `..Default`. |
| 8 | 4 | PAINFUL | Rename a field: `makemigrations` in a non-interactive shell fails with a misleading error. The docs never mention rename detection, the prompt, or `--yes` (README says "rename detection" only in a feature table). | hint about rename, or detection | `Error: Cannot add NOT NULL column "member"."full_name" without a default: existing rows would violate it. Add #[field(default = "...")] or make it Option<T>.` | Mention `--yes` and the rename prompt in makemigrations.md; on non-TTY, print "did you rename name -> full_name? rerun in a terminal or with --yes". With a TTY or `--yes` the generated SQL (`RENAME COLUMN`) was correct. |
| 9 | 4 | MINOR | `makemigrations.md` "What gets detected" omits renames, unique-constraint changes, index changes and the SQLite table rebuild appears only as a side note. The rebuild for the `Loan` unique constraint was correct (create new, copy, drop, rename, recreate indexes) and `migrate` worked. | table lists everything | n/a | Complete the table. |
| 10 | 2/12 | DOC-WRONG | field-types.md says `FieldDecimal<P,S>` -> `NUMERIC(P, S)` on Postgres; the generated SQL is plain `NUMERIC` (`\d book`: `numeric`), no precision/scale. Also `FieldVarchar<1,200>` bounds are not enforced on SQLite (stated). | `NUMERIC(8,2)` | n/a | Fix the doc or the generator. |
| 11 | 6 | MINOR | On SQLite a `FieldDecimal` returns as `BigInt(5)` for one row and `Double(7.5)` for the next in `values()` (dynamic typing); on Postgres it is `Decimal(5.00)`. `sum/avg` return `f64`, not Decimal: money sums are inexact. | Decimal | n/a | Document in values.md/aggregations.md. |
| 12 | 6 | MINOR | Keyset pagination per pagination.md (`.after(ID, x)` + `.order_by(ID)`) emits `ORDER BY "id" ASC, "id" ASC`. Harmless but sloppy; doc example forces it. | single ORDER BY | n/a | Dedupe, or drop `.order_by` from the example. |
| 13 | 11 | MINOR | SQLite unique violations carry no constraint name: `UniqueViolation { constraint: None }`, Display = `unique violation`. A beginner cannot tell which field clashed. On Postgres: `Some("author_email_key")` / `unique violation (uniq_loan)`. | name of the column | `UniqueViolation { constraint: None }` | Parse "UNIQUE constraint failed: author.email" on SQLite. |
| 14 | 11 | MINOR | Forgotten `migrate`: error is accurate but with no hint. | "did you run rango migrate?" | `database error 1: no such table: author` (Debug: `Database { code: Some("1"), message: "no such table: author" }`) | Add hint text when the SQLite/PG "no such table / relation does not exist" code is seen. |
| 15 | 10 | DOC-WRONG | tracing.md says sqlx emits `db.statement`; for single-line statements that field is the empty string, the text is in `summary` (`summary="SELECT COUNT(*) FROM \"book\"" db.statement=""`). The suggested setup `info,sqlx::query=warn` shows no queries at all; counting needs `sqlx::query=debug`. COMMIT/ROLLBACK are logged on Postgres, not on SQLite. | copy-paste setup shows queries | n/a | Document `sqlx::query=debug`, and `summary`; give a "count queries in tests" snippet. |
| 16 | all | PAINFUL | SQLite backend lacks several documented APIs and nothing says so: `bulk_update`, `bulk_upsert`, `update_or_create`, `QueryBuilder::transaction`, `atomic_retry` (this one is documented PG only) are absent from `rango::sqlite`; crud.md/transactions.md/bulk.md show them without a backend caveat (crud.md also passes `&mut pool`). | per-backend support table | n/a (cargo doc listing) | Add a feature/backend matrix to sqlite.md. |
| 17 | 5 | MINOR | `ForeignKey<M>: Clone` requires `M: Clone` (derive bound), so `l.book_id.clone()` fails unless the whole model derives Clone. | Clone always | `the method clone exists for struct ForeignKey<Member>, but its trait bounds were not satisfied ... consider annotating models::Member with #[derive(Clone)]` | Manual `Clone` impl without the bound. |
| 18 | 6 | DOC-WRONG | filter.md says `.one()` returns `Option<WithRelated<M>>`; rustdoc and compiler say `Option<M>`. Also filter.md `.explain()` example shows `"myapp_user"` (prefix removed in 0.2). Crate-level rustdoc still says `rango-orm = "0.1"`. `CheckConstraint::raw` (docs) vs `CheckConstraint::new` (AGENTS only; not tested). constraints.md says partial unique is "Postgres only"; not tested. | accurate | n/a | Sweep. |
| 19 | 11 | MINOR | Wrong column typo (`Book::NAME`) gives the generic `no associated function or constant named NAME found for struct Book`, with unrelated trait candidates (`block_buffer::sealed::Sealed`, `icu_properties...`). Understandable but noisy. The string form `eq("titel", ..)` fails at runtime (see below). | did-you-mean | see section "Mistakes" | Hard to improve; advise typed columns first in the quick-start (it teaches string names). |
| 20 | 2 | MINOR | `quick-start.md` teaches string column names (`.eq("active", true)`) and `models` docs say typed columns are better: a beginner will learn the weaker form first. | typed first | n/a | Switch quick-start to `User::ACTIVE`. |

Also noted, not a defect: the CLI must be installed separately and `rango migrate` needs `DATABASE_URL` each time (no `.env` loading). SQLite creates the file on `migrate`, which is nice. `PRAGMA foreign_keys=ON` is set by `connect_sqlite`, so cascade works out of the box (verified).

### Mistakes (step 11), exact output

1. Wrong column name, typed (`Book::NAME`): compile error `E0599: no associated function or constant named NAME found for struct Book in the current scope` - clear for a Rustacean; the candidate list is noise.
2. Wrong column name, string (`eq("titel","x")`): runtime `Invalid("unknown column \`titel\` on table \`book\`")` - good, before any SQL runs. A beginner understands it.
3. Wrong value type (`lt(Book::PRICE, "cheap")`): `E0277: &str cannot be compared to a rango::FieldDecimal<8, 2> column ... expected a rango::FieldDecimal<8, 2> value or its plain Rust equivalent`. Excellent message. `eq(Author::NAME, ..)` on Book: `trait FilterColumn<Book, _> is not implemented for Column<Author, FieldVarchar<1,100>>` - correct but harder to read.
4. Forgot migrate: `database error 1: no such table: author` (no hint, see #14).
5. Unique violation: see #13 (SQLite: no field name; PG: `author_email_key`). FK violation on SQLite: `ForeignKeyViolation { constraint: None }`.

## (b) Per-step lines of Rust and SQL query count

Lines are approximate (rustfmt style, from my main.rs); shared builders (author/book/member/loan helper fns) = 37 lines, models = 47 lines, counter layer = 32 lines. Query counts are data statements seen on target `sqlx::query` (BEGIN not logged; COMMIT/ROLLBACK logged only on PG and excluded below). The one-off `PRAGMA ...` on first connection was excluded from SQLite step 5.

| Step | Rust lines | SQL (SQLite) | SQL (Postgres) | Hand-written equivalent | Verdict |
|---|---|---|---|---|---|
| 5 CRUD (insert x2, get, update, delete, get) | ~16 (+ builders) | 6 | 6 | 6 | OK. `update` writes all columns, `RETURNING *` used, no re-fetch. Cascade verified. |
| 6 Queries (10 queries: author, price, or/not, exists, count, sum, avg, offset, keyset x4, values) | 54 | 13 | 13 | 13 | OK. `where_exists` is one correlated EXISTS. count/sum/avg are separate queries (as by hand). |
| 7 Relations (select_related x2 FKs; M2M add x2, remove, all; prefetch M2M; prefetch reverse FK) | 26 | 11 | 11 | 11 (2 tag inserts included) | OK. `select_related` on two FKs = ONE query; prefetch = 1 extra query each. No N+1. |
| 8 Transaction (borrow, borrow again -> abort, rollback demo) | 49 (31 for `borrow`) | 6 | 6 (+COMMIT, 2x ROLLBACK) | 6 | OK, rollback verified, `Error::abort` works. Check-then-insert is not race-proof by itself on PG (needs the DB constraint). |
| 9 Bulk 1,000 books | 10 | 9 | 1 (UNNEST) | 9 (SQLite chunks) / 1 | OK on count, but SQLite not atomic (#1). |
| 11 Mistakes + composite unique + bulk failure + member delete | 72 | 10 | 8 | n/a | n/a |

No step issued more queries than a hand-written version. No N+1, no re-fetch after insert/update.

Step 12 (dump/restore): `rango dump -o f.json` -> `rango restore f.json -d <fresh sqlite>` restored 1263 rows with identical per-table counts. Restoring a Postgres dump into a fresh SQLite file also worked (1015 rows, counts identical). `backup` wrote a 0600 gzip file. Restoring onto a non-empty DB fails cleanly on the unique constraint (nothing restored). Docs matched behaviour here.

Step 4 verdict: `ADD COLUMN pages` correct; rename = `RENAME COLUMN` correct (needs TTY or `--yes`, see #8); composite unique on SQLite = table rebuild, correct, `--check` then reports "Nothing to migrate". On Postgres the same models produced a single clean migration. Yes, it did the right thing.

## (c) What felt genuinely simple (and what did not)

Simple:
- `rango init` -> structs -> `makemigrations` -> `migrate`: the readable SQL file is correct, indexes on FKs and cascade are created for me. First run took minutes.
- Typed columns: `Book::filter(pool).lt(Book::PRICE, Decimal::new(1500,2)).order_by(Book::PRICE)` read like Django, and wrong-type errors are excellent.
- `where_exists`, `select_related` (two FKs, one query), `prefetch_related` (M2M and reverse FK), keyset pagination: I did not have to think about SQL at all; `SELECT` shapes were what I would have written.
- Same code ran on SQLite and Postgres with only the pool type and the raw-SQL placeholders changing.
- dump/restore across backends just worked.
- Error enum (`UniqueViolation`, `Aborted`) is easy to match.

Not simple:
- Building values is verbose (4 builder helpers, `FieldVarchar::new(..).unwrap()` everywhere, `ForeignKey::new(a.id.0)`, `ManyToMany::empty()`), nothing like `Author.objects.create(name=..)`.
- Everything is `FieldX(pub ..)`: `.0` all over (`b.title.0`).
- Transactions: the builder cannot run inside `atomic()`, so the one place correctness matters pushes me to raw SQL (#2).
- UUID-only FKs / M2M (docs): fine, but no auto-increment option with relations.
- Docs say some things that are wrong or backend-specific without saying so (#3, #4, #10, #16).

## (d) Doc pages wrong, missing or misleading

- getting-started/quick-start.md: `connect_sqlite` not in prelude (#4); uuid/chrono not in dependencies (#6); teaches string columns first (#20); `anyhow` listed but `tokio` macros assumed.
- getting-started/installation.md and crate rustdoc: version `0.1`.
- operations/bulk.md: `bulk_create(&mut **tx, ..)` does not compile on SQLite (#3); no word on SQLite non-atomic chunks (#1); `bulk_update`/`bulk_upsert` absent on SQLite (#16).
- operations/transactions.md: `QueryBuilder::transaction()` absent on SQLite; no mention that the query builder, M2M and bulk cannot run in a tx (#2, #16).
- operations/crud.md: `update_or_create(&mut pool, ..)` (absent on SQLite, `&mut pool` odd) (#16).
- operations/tracing.md: `db.statement` empty / `summary`; setup string hides queries (#15).
- migrations/makemigrations.md: renames, `--yes`, the TTY prompt, unique/index diffs missing (#8, #9).
- models/relations.md: no way to construct a `ManyToMany` field (#5); ForeignKey `Clone` bound (#17).
- models/field-types.md: `NUMERIC(P,S)` is not what is generated on Postgres (#10); SQLite Decimal behaviour in `values()` not mentioned (#11).
- models/constraints.md: partial-unique "Postgres only" vs AGENTS (SQLite supported); `CheckConstraint::raw` vs `new` (not verified).
- querying/filter.md: `.one()` return type; `myapp_user` table names (#18).
- querying/pagination.md: duplicate ORDER BY (#12).
- querying/aggregations.md: sum/avg return `f64`, not Decimal; say so for money.
- backends/sqlite.md: needs a supported-API matrix (#16).

## (e) Verdict

Does "you don't need to be a SQL expert" hold? Mostly yes for the schema and reading data; not fully for writes that must be correct under failure.

Score: 7/10.

Why not lower: I built the whole app, with a correct migration flow (including a rename and a composite unique on SQLite) and no N+1 anywhere, without writing a single `SELECT` or `CREATE TABLE` by hand. The query count equals hand-written everywhere, and compiler errors for the common typos are good.

Why not higher: (1) the one place a beginner needs SQL is the transaction ("fail if already borrowed"): the builder does not work in `atomic()`, so I had to write raw SQL with backend-specific placeholders; (2) SQLite `bulk_create` silently leaves partial data on failure; (3) the docs show APIs that do not compile or do not exist on SQLite; (4) creating model values is verbose and under-documented (`ManyToMany::empty()`, validated vs unvalidated constructors, uuid/chrono deps); (5) the rename flow fails with a misleading message when not on a TTY.

## (f) Decisions

Checked against the tree at commit cd9a7cd (2026-11-08).

| # | Decision | Commit or reason |
|---|---|---|
| 1 | fixed | edbecc8: chunked bulk operations run in one transaction (docs/src/operations/bulk.md, Atomicity). |
| 2 | fixed | 31908b1: query builder, M2M and bulk accept a pool or a transaction on all backends; docs ad3c578. |
| 3 | fixed | 31908b1: `bulk_create(&mut **tx, ..)` now compiles on SQLite; docs match. |
| 4 | fixed | 903f75a: `connect_sqlite` is in the prelude. |
| 5 | fixed | c8c74bc: relation types no longer need bounds on `M`, so `ManyToMany::default()` works; documented in defining-models.md (5fb5fd5). |
| 6 | documented | bbe54d9: quick-start lists uuid and chrono as dependencies you add. Shorter constructors (`FieldUuid::new()`, `FieldDateTime::now()`) are still not shown in the examples. |
| 7 | fixed | ba80c4c: validated field values are private, so `FieldVarchar("x".into())` no longer compiles and validation cannot be skipped. Verbosity of building values: won't fix, by design (defining-models.md: "There is no generated constructor: the fields are the API"). |
| 8 | fixed | 1c25338: non-TTY error mentions a possible rename and `--yes`; renames and `--yes` documented in migrations/makemigrations.md (5fb5fd5). |
| 9 | open | The "What gets detected" table in docs/src/migrations/makemigrations.md still has no rename, constraint or index rows (renames and the SQLite rebuild have their own sections). |
| 10 | documented | 5fb5fd5: field-types.md now says Postgres and SQLite use a plain `NUMERIC`, only MySQL/MariaDB enforce P and S. Generator unchanged. |
| 11 | documented | 5fb5fd5: aggregations.md and field-types.md say `sum`/`avg` are `f64`. SQLite dynamic typing of Decimal in `values()` is still not mentioned in values.md. |
| 12 | fixed | bbe54d9: `.after()` orders by the column once; a repeated `.order_by` on it is not duplicated. |
| 13 | fixed | 3980e1d: SQLite unique violations carry the failing columns (`author.email`). |
| 14 | fixed | 3980e1d: typed `Error::TableMissing { table }`; errors.md says to run makemigrations and migrate. The Display text itself has no hint. |
| 15 | fixed | 5fb5fd5: tracing.md documents `summary`, the empty `db.statement`, and `sqlx::query=debug`. The "count queries in tests" snippet was not added. |
| 16 | fixed | 31908b1, 5ea0750: bulk_update, bulk_upsert, update_or_create and `transaction()` exist on SQLite. `atomic_retry` is Postgres/MySQL only and transactions.md says SQLite has no equivalent. |
| 17 | fixed | c8c74bc: manual `Clone`/`Copy` for `ForeignKey<M>` without a bound on `M` (rango-orm-core/src/fields.rs). |
| 18 | open | Fixed: `.one()` type, `myapp_user` sample, crate version (0.2) (bbe54d9, fd2d824). Still wrong: constraints.md and defining-models.md use `CheckConstraint::raw`, which does not exist (only `CheckConstraint::new`); constraints.md says partial unique is "Postgres only" while the CLI emits it for SQLite too. |
| 19 | won't fix | Compiler candidate noise on a mistyped constant comes from rustc, not Rango. c8c74bc added clearer messages elsewhere, and run 2 saw a "did you mean" help. |
| 20 | open | quick-start.md still teaches string column names (`.eq("active", true)`) before typed columns. |
| D | see rows | Section (d) repeats rows above: quick-start #4 #6 #20, bulk #1 #3 #16, transactions #2 #16, crud #16, tracing #15, makemigrations #8 #9, relations #5 #17, field-types #10 #11, constraints #18, filter #18, pagination #12, aggregations #11, sqlite #16. Extra items: installation.md version `0.1` fixed (5fb5fd5); "anyhow listed but tokio macros assumed" not checked. |
