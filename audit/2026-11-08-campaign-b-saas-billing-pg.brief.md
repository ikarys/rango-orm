# Blind test brief, campaign B

**Persona:** a backend engineer who knows SQL and PostgreSQL well. You check the generated SQL, the query count and the transaction semantics, and you distrust ORMs that hide them.

**App:** the data layer of a multi-tenant SaaS billing API. Tenants, customers, products, invoices, invoice lines, payments; money is exact decimals; an invoice number is unique per tenant; issuing an invoice (number allocation + lines + total) is atomic; two workers paying the same invoice concurrently must not double-pay; customer metadata is JSON and searchable; invoice lists are paginated by keyset for an API; a dashboard needs revenue per tenant per month; the API returns slim projections, not whole rows.

**Backend:** PostgreSQL 17 (`postgres://dev:dev@127.0.0.1:5432/blind_b`, create it with `docker exec -i devpg psql -U dev -d postgres -c 'CREATE DATABASE blind_b'`). Set up a separate migration role and a DML-only app role as the docs describe (you may use the superuser `postgres`/`postgres` only to create roles).

**Campaign features (from the coverage table):** installation, quick start, configuration; defining models, field types; ForeignKey, ManyToMany; constraints, ordering, partial unique index; makemigrations (add, rename, constraint change); migrate and the least-privilege migration role (`MIGRATE_DATABASE_URL`); filters, `or` / `not`, subqueries (`where_exists`, `in_subquery`); aggregations, decimals; select_related, prefetch_related; partial selects (`select_as`); keyset pagination; CRUD, get_or_create, update_or_create; bulk create / update / upsert; transactions, atomic_retry under a real conflict; errors; tracing, slow query log, `.explain()` / `.explain_analyze()`; backend extensions (JSON, full-text search); TLS and credentials (what you would configure for production; you cannot reach a remote host, so check what the docs promise and what `DatabaseConfig` exposes); export / import, dump / restore / backup.

## Rules (read twice)

You are testing whether Rango, an async Rust ORM, can be used from its documentation alone. Follow the protocol in the user guide page `docs/src/trust/blind-tests.md`; this brief adds the concrete setup.

- Allowed: the mdBook user guide (`/home/tony/workspace/rango-orm/docs/src/`, read the Markdown files directly or run `mdbook build`), `/home/tony/workspace/rango-orm/README.md`, `cargo doc --open`-style output for `rango-orm` built from YOUR app (`cargo doc -p rango-orm --no-deps`, read the HTML or use `--document-private-items` off), compiler and runtime output.
- Forbidden: any other file under `/home/tony/workspace/rango-orm/` (source, tests, examples, `AGENTS.md`, `CHANGELOG.md`, `audit/`, git history). Repo paths printed in compiler errors are not followed. If only the source would unblock you, log `BLOCKER: needed source` and move on.
- Allowed deviations (not friction): depend on Rango by path (`rango-orm = { path = "/home/tony/workspace/rango-orm/rango-orm", ... }`) and build the CLI from the repo (`cargo install --path /home/tony/workspace/rango-orm/rango-orm-cli --root <your app>/.tools` or `cargo run --manifest-path ...`). The docs say `rango-orm = "0.2"`; the repo is a pre-release, so the path dependency replaces it.
- Real databases, no mocks. Log friction as it happens, in a scratch file, not from memory at the end.
- Build a real app: a library crate with the models and the operations, plus a binary or tests that run every step end to end. Keep it in `/tmp/rango-blind-b`. It must still build and run when you finish.
- Do not modify anything under `/home/tony/workspace/rango-orm/`. Drop any PostgreSQL database you created when done (`docker exec -i devpg psql -U dev -d postgres -c 'DROP DATABASE ...'`); for MySQL/MariaDB drop your databases too.

## Steps

1. Install, `rango init`, configure the connection.
2. Write the models: primary keys, FKs, many-to-many, optional fields, constraints.
3. `makemigrations` + `migrate`.
4. Evolve the schema: add a field, rename a field, add a constraint, then migrate again.
5. Seed with bulk operations (at least 1,000 rows).
6. Write the app's real queries: filters, relations, aggregates, pagination.
7. One transaction scenario where atomicity matters.
8. A concurrency or retry case where the backend supports it.
9. Observability: count the SQL queries of each step from the tracing output.
10. Deliberate mistakes; judge each error message: forgot `migrate`, value of the wrong type, unique violation, FK to a missing row, unknown column name passed as a string.
11. Dump, restore into an empty database, back up.

Also exercise every feature marked for your campaign in the "Feature coverage" table of `blind-tests.md` (listed again below). If a feature does not fit the app naturally, use it anyway in a small realistic way and say so.

## Report

Write the report to `/tmp/rango-blind-b/REPORT.md` in exactly the format of the "Report format" section of `blind-tests.md`: header (date 2026-11-08, Rango commit `bd5de84`, database versions, persona, tester model), then (a) friction log table, (b) per step lines of Rust and SQL query count, (c) what felt simple, (d) doc pages wrong, missing or misleading, (e) verdict with score /10 and whether you would use Rango for this app. Leave (f) Decisions out: the maintainer writes it. Severity: BLOCKER, PAINFUL, DOC-WRONG, MINOR. Exact error text is copied, not paraphrased. Be demanding and concrete: a finding without a reproduction step is not useful. Do not pad with praise.

Final message: path of the report, score, counts per severity, and the BLOCKER and DOC-WRONG findings one line each.
