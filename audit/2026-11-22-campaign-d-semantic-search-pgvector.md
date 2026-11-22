---
date: 2026-11-22
commit: df14ac8 (Rango, path dependency; CLI built from the repo with cargo install --path)
kind: blind-campaign-d
tool: rango-orm 0.3.1 + rango CLI, semantic search with pgvector
model: claude-sonnet-5-5 (vibecoding-blind-test agent)
scope: installation, models/field types, pgvector (FieldVector, Option<FieldVector>, #[field(index = "...")], nearest, within, all_with_distance, filter + order_by), makemigrations/migrate, bulk_create, tracing, explain, errors
verdict: 7/10, would use Rango for this app with caveats
---

## Brief

Persona: Rust backend developer adding semantic search to a notes app (knows Rust and SQL, used pgvector by hand once, never used Rango). Database: PostgreSQL 16.16 (Docker image, port 5433), pgvector 0.8.7, database `blind_d` (dropped at the end; `rango_test` untouched). Embeddings are NOT real: deterministic pseudo-embeddings (hashed bag of words over 384 dimensions, L2-normalised), generated in Rust; real embeddings are out of scope. Dataset: 20,000 notes (8 words each from a 600-word vocabulary, ages 0 to 89 days, every second row has the optional embedding).

Blindness: the only Rango files read were the brief, `docs/src/trust/blind-tests.md`, `docs/src/**` and `README.md`. Rango was used as a path dependency and the CLI built with `cargo install --path ... --root /tmp/rango-blind-d/.tools`. I never opened source, tests, examples, audit, CHANGELOG or git history. Compiler errors print repo paths (`rango-orm-core/src/vector.rs`, `rango-orm-postgres/src/pg_ext.rs`); I did not follow them.

App: `/tmp/rango-blind-d` (lib `notes_search` with `models.rs` + search functions in `lib.rs`; binaries `run`, `explain`, `extra`, `mistake_rt`, `bulk`). It builds with `cargo build --release`; the database is dropped, so the binaries need a re-created, migrated database to run. Friction scratch log: `/tmp/rango-blind-d/FRICTION.log`.

Measured (release build, SQL statements counted from `sqlx::query` events via a custom tracing layer):

| Search | SQL stmts | Wall time | Plan (`explain_analyze`) |
|---|---|---|---|
| top 5 + distance (cosine) | 2 on first call, 1 afterwards | 3.1 ms first, plan exec 0.25 ms | HNSW `Index Scan using note_embedding_hnsw` |
| radius `within` (0.5, limit 50) | 1 | 5.1 ms (0.2 radius: 20 ms) | `Seq Scan`, no index |
| filter (30 days) + nearest + order_by created_at desc | 1 | 9.5 ms (plan exec 13 ms) | `Seq Scan` + top-N sort, no HNSW |
| optional embedding (L2), 10,000 NULL rows | 1 | 1.5 ms | HNSW `note_embedding_b_hnsw` |
| optional embedding, cosine on the L2 index | 1 | 7 ms | Seq Scan (documented) |
| L2 `nearest` on the cosine-indexed `embedding` | 1 | plan exec 18.7 ms | Seq Scan (documented) |

A note searched with its own text came first at distance 0.000000; distances were sorted ascending and plausible (0.1 for notes sharing 7 of 8 words).

Bulk create: 20,000 rows (2 HNSW indexes + 1 more, 384-d) in 11.78 s = 1,698 rows/s in one `bulk_create` call (4 SQL statements logged); 5,000 rows 1.28 s (3,900 rows/s).

## (a) Friction log

| # | Step | Severity | What happened | Expected | Exact error | Suggested fix |
|---|---|---|---|---|---|---|
| 1 | 6 (first search) | DOC-WRONG | `pgvector.md` says `FieldVector::<N>::new(Vec<f32>) -> Result<Self, String>`. The error type is `FieldError`, not `String`. A function written as `.map_err(Error::Invalid)?` does not compile. (`?` straight into `rango::Error`/`anyhow` works.) Repro: `FieldVector::<384>::new(v).map_err(Error::Invalid)?` | Doc matches the signature | `error[E0631]: type mismatch in function arguments ... expected function signature fn(FieldError) -> _ found function signature fn(String) -> _` | Fix the signature in `backends/pgvector.md` (also for `TryFrom`); say `FieldError` converts to `rango::Error` with `?` |
| 2 | 3 (migrations) | PAINFUL | Bare `#[field(index)]` on a vector field: docs say it is a compile error. `rango makemigrations` accepts it and writes a b-tree index on the vector column (`CREATE INDEX IF NOT EXISTS "d_e_idx" ON "d" ("e");`); only `cargo build` refuses. Since `makemigrations` is run first in the documented flow, the user gets a useless b-tree index. Repro: `#[field(index)] pub e: FieldVector<4>`, `rango makemigrations`, read `migrations/0001_d.sql` | `makemigrations` refuses it like the derive does | none from makemigrations; cargo says `error: a vector field needs a distance for its index: #[field(index = "cosine")], "l2" or "inner_product"` | Reject a bare index on `FieldVector` in `makemigrations`, as it already does for `index = "euclid"` and `index = "cosine"` on a text column |
| 3 | 7 (filtered search) | PAINFUL | Filter + `nearest` + `order_by`: Postgres uses `Seq Scan` + top-N sort, never the HNSW index (30 days = 1/3 of rows; also with 2 days = 2%). The docs say "It combines with any filter" and promise "still uses the HNSW index", without saying a filter usually defeats the index (when it does use it, HNSW post-filters and can return fewer than k rows; pgvector's `hnsw.ef_search` / iterative scan). The task asks to check the index is used for the filtered search: it is not, and the page gives no hint what to do (partial index, `ef_search`, how to set a session parameter via `after_connect`). `within` is also always a Seq Scan (`WHERE col <=> $1 < max`), which is not said either. | A caveat paragraph: filtered and radius queries are exact scans unless Postgres picks the index; what to do about it | none (plan: `Seq Scan on note ... Filter: (created_at >= ...)`) | Add a "Filters, radius and the index" section with the plan outcome, the post-filter risk, and the `after_connect` hint for `SET hnsw.ef_search` |
| 4 | 6 (`order_by` tie-break) | MINOR | "A later `.order_by(...)` only breaks ties" is true but with float distances exact ties are rare: my 5 results at "0.100000" were not sorted by date (14960 on 11-02, 901 on 11-21 ...) because the distances differ in the 8th decimal. A user expecting "tie-broken by date" gets no visible tie-break. | A note that ties only occur on exactly equal distances (duplicates, rounded embeddings) | none | One sentence on this in `pgvector.md` |
| 5 | 7 (`.explain()`) | DOC-WRONG (small) | The brief and `blind-tests.md` say "check `.explain()` uses an index". `.explain()` returns only the SQL (`SELECT * FROM "note" ORDER BY "embedding" <=> $1::vector LIMIT 5`); the plan needs `.explain_analyze()` which runs the query. `filter.md` explains it correctly, but the pgvector page, which promises "Postgres uses the HNSW index" and "the benchmark checks that with EXPLAIN ANALYZE", never shows how a user checks it. | pgvector page shows the 3-line index check | none | Add `explain_analyze` snippet to `pgvector.md`; fix the wording "`.explain()` shows the index" in `blind-tests.md` step 7 |
| 6 | 7 (`explain_analyze`) | MINOR | The plan text embeds the whole 384-float vector literal on the `Order By`/`Sort Key`/`Filter` lines (about 1 KB per line). I had to truncate lines to read it. | Truncation or note that the vector is inlined | `Order By: (embedding <=> '[0,0,0,...` | Mention it in the docs, or shorten the printed literal |
| 7 | 7 (tracing) | MINOR | Statement counts are off by one on the first use of each connection: the first `top_k` logged 2 statements, `bulk_create` 4 (type lookups `SELECT pg_type.oid, pg_type.oid::regtype::text pretty_name, ...` for `vector`/`vector[]`, one per pooled connection used). `tracing.md` only warns about `after_connect` and MySQL `sql_mode`; a warm-up with `Note::filter(&pool).limit(1).all()` did not cover every type or connection. Repro: count `sqlx::query` events around a first `nearest` on a fresh pool. | The doc names the type-lookup statement and says to warm every pooled connection | `SELECT pg_type.oid, pg_type.oid::regtype::text pretty_name, …` (summary field) | Add the pgvector type lookup to the "Counting statements" paragraph |
| 8 | 8 (mistakes) | MINOR | `nearest("title", &q, 5, Distance::Cosine)` (string column name) does not compile, although `filter.md` says "String names ... are still accepted everywhere". `pgvector.md` does say a column constant is required, so it is a gap in the general sentence. The message is good. | "everywhere" qualified | `error[E0277]: `&str` is not a vector column of `Note`` ... `pass a `FieldVector<N>` column constant such as `Doc::EMBEDDING`, and a query `&FieldVector<N>` of the same `N`` | Say in `filter.md` that vector methods need constants |
| 9 | 8 (mistakes) | MINOR | `FieldVector<16001>` gives a misleading message; `pgvector.md` promises "the derive refuses other values" but the message does not name the range. Repro: `pub e: FieldVector<16001>` in a model | `N must be in 1..=16000` | `error: Invalid FieldVector syntax: `FieldVector<16001>`` | Message with the allowed range |
| 10 | 8 (mistakes) | MINOR | `nearest(.., k = -1, ..)` surfaces a raw Postgres error as `Error::Database`; `k = 0` silently returns 0 rows. Neither is `Error::Invalid`, unlike the other input checks. | `Error::Invalid` for `k <= 0` | `Err(Database { code: Some("2201W"), message: "LIMIT must not be negative" })` | Validate `k` before running SQL (k is `i64`, not `usize`, which is itself surprising) |
| 11 | 8 (mistakes) | MINOR | Extension missing but table present (extension not created in that database) surfaces as a decode error on insert, not as a hint to run the migration containing `CREATE EXTENSION`. Repro: create a table by hand in a DB without pgvector and `bulk_create` a note. | A message naming pgvector | `Error: decode error: type named vector not found` | Map "type named vector not found" to a hint about the `vector` extension and `rango migrate` |
| 12 | 5 (seed) | MINOR | 20,000 rows take 11.8 s (1,700 rows/s), 5,000 take 1.3 s (3,900 rows/s): throughput falls as the table grows because three/two HNSW indexes are updated per row. Docs say nothing about bulk-loading vectors (create the index after load, `maintenance_work_mem`). | A hint for big loads | `WARN slow statement: execution time exceeded alert threshold` (5,000-row insert) | One paragraph in `pgvector.md` on large loads |
| 13 | 3 (environment) | MINOR (not Rango) | The brief's `CREATE DATABASE blind_d` failed on this Docker image (template1 collation version mismatch); I used `TEMPLATE template0`. Not counted against Rango. | | `ERROR:  template database "template1" has a collation version mismatch` | |

Extra, one line: `pgvector.md` quick example and the docs never show `Option<FieldVector<N>>` with `nearest` (it works; NULL rows are skipped, checked: 10,000 NULL rows, query returned 5 non-NULL rows with an HNSW scan); `within` returns at most 1000 rows by default (the general rule from `filter.md`, not repeated on the pgvector page); `.count()` with `nearest` returned 20000 (documented).

## (b) Effort per step

| Step | Lines of Rust written | SQL queries made |
|---|---|---|
| 1 install, init, config | 0 (Cargo.toml: 7 lines) | 0 |
| 2 model | 12 (`models.rs`) | 0 |
| 3 makemigrations + migrate | 0 | 1 migration (extension, table, 2 HNSW indexes) |
| 4 evolve schema (index -> l2, third column) | 4 | 2 migrations (drop+create index; add column + index) |
| 5 seed 20,000 | about 12 | 4 SQL statements for one `bulk_create` |
| 6 four searches | about 28 (`lib.rs` search functions) | 1 each (2 on first use) |
| 7 volume, explain, tracing counter | about 60 (`run.rs`, `explain.rs`, `extra.rs`; 14 lines are the counting layer) | 1 per search + EXPLAIN ANALYZE |
| 8 deliberate mistakes | about 40 in 3 scratch programs | 2 runtime queries |

Hashed bag-of-words embedding: 17 lines (out of scope, in `lib.rs`).

## (c) What was simple

- From docs to a working top-5 with distance: model (6 fields), `rango makemigrations`, `rango migrate`, one `nearest(...).all_with_distance()` line. No cast, no raw SQL, no `CREATE EXTENSION` by hand. This matched the persona's expectation.
- The generated SQL is what I would have written by hand: `CREATE EXTENSION IF NOT EXISTS vector;`, `vector(384)` columns, `USING hnsw (... vector_cosine_ops)`, `vector_ip_ops`, `vector_l2_ops`.
- Changing the distance of an index gave `DROP INDEX IF EXISTS` + new `CREATE INDEX` with the right operator class; a new nullable vector column gave `ADD COLUMN` + index. Both in one `makemigrations` run each.
- `Option<FieldVector<384>>` works for the optional embedding including `nearest` on it, with an HNSW scan.
- Compile-time checks are real: a `FieldVector<10>` query for a 384 column (`expected `384`, found `10``), and `Note::TITLE` as a vector column both fail with a readable error that names the fix.
- Runtime messages for the value mistakes are clear: `Expected 384 dimensions, got 10`, `Vector values must be finite` (NaN and infinity), `all_with_distance() needs nearest() in the query`, and `table `note` does not exist — run `rango makemigrations` and `rango migrate`` for a forgotten migrate. Wrong distance names are rejected with the list of valid values in both `makemigrations` and the compiler.
- `explain_analyze` on a vector query worked on the first try and showed `Index Scan using note_embedding_hnsw`.
- Distances in `all_with_distance` are sorted and exact: own text at 0.000000.

## (d) Documentation pages wrong, missing or misleading

- `backends/pgvector.md`: wrong error type of `FieldVector::new` (#1); says a bare `#[field(index)]` is a compile error but `makemigrations` accepts it (#2); no word that filters and `within` do not use the HNSW index and on the post-filter risk (#3); no example of the index check with `explain_analyze` (#5); no note on exact ties (#4) or on large loads (#12); no mention of the first-use `pg_type` statement in counts (#7, also `operations/tracing.md`).
- `querying/filter.md`: "String names ... accepted everywhere" is not true for `nearest`/`within` (#8).
- `trust/blind-tests.md` step 7: "`.explain()` uses an index" is wrong, the plan comes from `.explain_analyze()` (#5).
- `README.md`: the Features table has no pgvector row, so a reader looking for vector search from the README does not find it (and `rango-orm = "0.3"` in the install snippet: I used a path dependency and could not check that the registry 0.3.x includes pgvector).

## (e) Verdict

Score: 7/10. I would use Rango for this app: the common path (model, index, migration, top-k with distance) is a few lines with no cast or raw SQL, and the generated SQL is correct, which is better than I expected for a vector ORM layer. The score is held down by the filtered search case, which is the most common production shape (filter plus nearest): it silently runs as an exact scan and the docs promise the HNSW index without saying when it is lost, by `makemigrations` accepting what the compiler refuses, by the wrong signature in the one doc page that matters, and by small error-message gaps (k, dimension range, missing extension).

BLOCKERs: none. DOC-WRONG: 2 (#1, #5). PAINFUL: 2 (#2, #3). MINOR: 9 (#4, #6 to #13).

## (f) Decisions

Written by the maintainer.

| # | Decision | Commit or reason |
|---|---|---|
| 1 | fixed | `pgvector.md` gives `Result<Self, FieldError>` and says `?` converts it to `Error::Invalid`. |
| 2 | fixed | `rango makemigrations` refuses a bare `#[field(index)]` on a vector field with the derive's message; test `vector_index_attribute_maps_to_its_distance` (CLI scanner). |
| 3 | documented | New section "When Postgres uses the HNSW index" in `pgvector.md`: pgvector filters after the index scan, `within` alone never uses the index, what to do for a selective filter (index on the filter column, `hnsw.iterative_scan`). The planner's choice is Postgres's; Rango sends the query pgvector documents. |
| 4 | won't fix | Correct behaviour: `order_by` after `nearest` only breaks exact ties, as documented. Sorting by date within a distance band is an app decision (`nearest` with a larger `k`, then sort in Rust). |
| 5 | fixed | `blind-tests.md` step 7 now says `.explain_analyze()`; `pgvector.md` says `.explain()` only returns the SQL. The wrong wording was in the protocol and the brief, not the API docs (`filter.md` already said `.explain()` returns SQL text). |
| 6 | won't fix | The plan text is Postgres's own `EXPLAIN` output; rewriting it would hide what Postgres printed. |
| 7 | won't fix | The `pg_type` lookups are sqlx resolving the `vector` type once per connection; they are real statements and the count is honest. |
| 8 | documented | `filter.md` and `pgvector.md` say the pgvector methods take the typed column only. The column type is what lets the compiler check the dimension. |
| 9 | fixed | The derive now says ``Invalid FieldVector: `FieldVector<16001>`, N must be in 1..=16000``; trybuild case `vector_dimension_range.rs`. |
| 10 | won't fix | `k` goes to `LIMIT` like `.limit(n)`: `k = 0` returns no rows, a negative `k` is Postgres's `LIMIT must not be negative` (2201W). Same behaviour as `.limit()`; no data or security impact. |
| 11 | documented | `pgvector.md` Setup names the error `decode error: type named vector not found` and its cause. |
| 12 | documented | `pgvector.md`: a large first load is faster before the index exists. |
| 13 | won't fix | Not Rango: a collation mismatch in the local Docker image. |
| extra | documented | `pgvector.md`: NULL vectors are never returned; `within` follows the 1000-row cap of `.all()`. The README features table now lists vector search. |
