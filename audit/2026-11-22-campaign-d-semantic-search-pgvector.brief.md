# Blind test brief, campaign D

**Persona:** a Rust backend developer adding semantic search to an existing app. You know Rust and SQL, you have used pgvector by hand once, and you have never used Rango. You expect vector search to take a few lines; every extra step, cast or raw SQL you need counts against Rango.

**App:** the search layer of a notes app. A `note` table: id, title, body, created_at, an embedding of 384 dimensions with a cosine HNSW index, and a nullable embedding for a second, optional model (inner product). Features: insert notes with their embedding; "top 5 notes closest to this query" with the distance shown to the user; "related notes" within a distance radius; the same search restricted by a filter (e.g. created in the last 30 days) and tie-broken by date; check that the HNSW index is used. Embeddings: you have no embedding model; generate deterministic pseudo-embeddings in Rust (e.g. a hashed bag of words, normalised), so that notes sharing words are close. Say so in the report; producing real embeddings is out of scope.

**Backend:** PostgreSQL 16 with pgvector, in Docker: `postgres://postgres:postgres@127.0.0.1:5433/blind_d`. Create the database with `docker exec -i rango-orm-postgres-1 psql -U postgres -d postgres -c 'CREATE DATABASE blind_d'` and drop it when done. Do not touch the `rango_test` database.

**Campaign features (from the coverage table):** installation, quick start, configuration; defining models, field types; vector search (pgvector): `FieldVector<N>`, `Option<FieldVector<N>>`, `#[field(index = "...")]`, the generated extension and HNSW index, `nearest`, `within`, `all_with_distance`, combination with filters and `order_by`; makemigrations (add a vector column, change the index distance); migrate; bulk create; tracing, `.explain()`; errors.

## Rules (read twice)

You are testing whether Rango, an async Rust ORM, can be used from its documentation alone. Follow the protocol in the user guide page `docs/src/trust/blind-tests.md`; this brief adds the concrete setup.

- Allowed: the mdBook user guide (`/home/tony/workspace/rango-orm/docs/src/`, read the Markdown files directly or run `mdbook build`), `/home/tony/workspace/rango-orm/README.md`, `cargo doc -p rango-orm --no-deps` output built from YOUR app, compiler and runtime output.
- Forbidden: any other file under `/home/tony/workspace/rango-orm/` (source, tests, examples, benchmarks, `AGENTS.md`, `CHANGELOG.md`, `audit/`, git history). Repo paths printed in compiler errors are not followed. If only the source would unblock you, log `BLOCKER: needed source` and move on.
- Allowed deviations (not friction): depend on Rango by path (`rango-orm = { path = "/home/tony/workspace/rango-orm/rango-orm", ... }`) and build the CLI from the repo (`cargo install --path /home/tony/workspace/rango-orm/rango-orm-cli --root <your app>/.tools`).
- Real database, no mocks. Log friction as it happens, in a scratch file, not from memory at the end.
- Build a real app in `/tmp/rango-blind-d`: a library crate with the model and the search functions, plus a binary or tests that run every step end to end. It must still build and run when you finish.
- Do not modify anything under `/home/tony/workspace/rango-orm/`.

## Steps

1. Install, `rango init`, configure the connection.
2. Write the `Note` model with the two vector fields and their index attributes.
3. `makemigrations` + `migrate`. Read the generated SQL: extension, column types, HNSW indexes.
4. Evolve the schema: change the optional embedding's index distance to `l2`, then add a third vector column; makemigrations + migrate again. Read the SQL.
5. Seed 20,000 notes with bulk create. Time it, report rows/s.
6. Write the searches: top 5 with distance, radius, filtered + tie-broken, the optional embedding (some rows NULL).
7. Volume: for each search report SQL statement count (from tracing), wall time, and whether `.explain()` shows the HNSW index. Check the distances returned are sorted and plausible (a note searched with its own text should come first, at distance ~0).
8. Deliberate mistakes; judge each error message: query vector of the wrong dimension (at `FieldVector::new` and at the call site), a non-vector column passed to `nearest`, a NaN in the vector, `all_with_distance` without `nearest`, a vector field with a bad index distance, forgot `migrate`.

## Report

Write the report to `/tmp/rango-blind-d/REPORT.md` in exactly the format of the "Report format" section of `blind-tests.md`: header (date 2026-11-22, Rango commit `df14ac8`, database version, pgvector version, persona, tester model), then (a) friction log table, (b) per step lines of Rust and SQL query count, (c) what felt simple, (d) doc pages wrong, missing or misleading, (e) verdict with score /10 and whether you would use Rango for this app. Leave (f) Decisions out: the maintainer writes it. Severity: BLOCKER, PAINFUL, DOC-WRONG, MINOR. Exact error text is copied, not paraphrased. Be demanding and concrete: a finding without a reproduction step is not useful. Do not pad with praise.

Final message: path of the report, score, counts per severity, and the BLOCKER and DOC-WRONG findings one line each.
