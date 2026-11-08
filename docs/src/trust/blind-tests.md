# Blind usability tests

## Why

A unit test checks that code does what its author meant. A blind test checks that a newcomer can use Rango from the docs alone: it finds wrong docs, missing docs, confusing errors and API gaps.

Rango is written with AI assistance, and so are its blind tests: the testers are AI agents started with a fresh context and only the user docs. That is a real limit, stated [below](#limits). The method and every report are public, so anyone can judge them or run the same campaigns.

## Rules for the tester

Allowed inputs:

- The mdBook user guide
- The README
- `cargo doc` output for the `rango-orm` crate
- Compiler and runtime output

Not allowed:

- Rango source code
- Rango tests
- `AGENTS.md`
- Git history
- Issues

Repo paths printed inside compiler errors are not followed. If the tester is stuck and only the source would help, it logs a `BLOCKER: needed source` entry and stops that step; each one is a doc bug.

Allowed deviations, not counted as friction:

- A path dependency on the repo
- The CLI built from the repo

Real databases only:

- A SQLite file
- A local PostgreSQL server
- MySQL in Docker
- MariaDB in Docker

No mocks. The tester logs friction as it happens, not from memory at the end.

## Campaigns

Three campaigns, each a different persona building a different real app. Together they touch every page of the user guide.

| Campaign | Persona | App | Backends |
|---|---|---|---|
| A | Django developer, little SQL | Library: books, authors, members, loans; one active loan per book | SQLite, then ported to PostgreSQL |
| B | Backend engineer, knows SQL well | Multi-tenant SaaS billing API: tenants, customers, invoices, invoice lines, money as decimals | PostgreSQL |
| C | Indie developer who ships on MySQL | Event booking with seat capacity and waiting list | MySQL 8 and MariaDB, SQLite for its tests |

Each campaign follows the same steps:

1. Install, run `rango init`, and configure the connection.
2. Write the models: primary keys, foreign keys, many-to-many relations, optional fields, and constraints.
3. Run `makemigrations` and `migrate`.
4. Evolve the schema: add a field, rename a field, add a constraint, then migrate again.
5. Seed data with bulk operations, at least 1,000 rows.
6. Write the app's real queries: filters, relations, aggregates, and pagination.
7. Run one transaction scenario where atomicity matters, for example borrow a book, book the last seat, or issue an invoice.
8. Run a concurrency or retry case where the backend supports it.
9. Check observability: count the SQL queries of each step from the tracing output.
10. Make deliberate mistakes and judge each error message: forgot `migrate`, value of the wrong type, unique violation, foreign key to a missing row, unknown column name passed as a string.
11. Dump, restore into an empty database, and back up.

## Feature coverage

| Feature (docs page) | A | B | C |
|---|---|---|---|
| Installation, quick start, configuration | x | x | x |
| Defining models, field types | x | x | x |
| Implicit primary key (`#[rango::model]`, bigint) |  |  | x |
| Relations: ForeignKey, ManyToMany | x | x | x |
| Model hooks | x |  |  |
| Constraints, ordering, partial unique index | x | x | x |
| makemigrations (add, rename, constraint change, SQLite rebuild) | x | x | x |
| migrate, least-privilege migration role (`MIGRATE_DATABASE_URL`) | x | x | x |
| Filters, `or` / `not`, subqueries (`where_exists`, `in_subquery`) | x | x | x |
| Aggregations, decimals | x | x | x |
| select_related, prefetch_related | x | x |  |
| values() |  |  | x |
| Partial selects (`select_as`) |  | x |  |
| Keyset pagination | x | x |  |
| Raw SQL, stored procedure via `raw` |  |  | x |
| CRUD, get_or_create, update_or_create | x | x | x |
| Bulk create / update / upsert | x | x | x |
| Transactions, atomic_retry | x | x | x |
| Errors | x | x | x |
| Tracing, slow query log, `.explain()` / `.explain_analyze()` | x | x | x |
| Backend extensions (JSON, full-text search) |  | x |  |
| TLS and credentials |  | x | x |
| export / import, dump / restore / backup, restore across backends | x | x | x |

When a docs page is added, it gets a row and at least one campaign.

## Report format

Each report is a Markdown file in [`audit/`](https://github.com/ikarys/rango-orm/tree/main/audit), named `YYYY-MM-DD-<campaign>.md`. The exact brief given to the tester is committed next to it, as `YYYY-MM-DD-<campaign>.brief.md`. The report starts with a header:

- Date
- Rango commit
- Database versions
- Persona
- The tester's model

Then it has these parts:

- (a) Friction log
- (b) Per step: lines of Rust written and number of SQL queries.
- (c) What felt simple
- (d) Doc pages wrong, missing, or misleading
- (e) Verdict: score out of 10, and whether the tester would use Rango for this app.
- (f) Decisions, written by the maintainer, not the tester

The friction log is a table:

| # | Step | Severity | What happened | Expected | Exact error | Suggested fix |
|---|---|---|---|---|---|---|

Severity is one of:

- `BLOCKER`: could not continue
- `PAINFUL`: worked around with effort
- `DOC-WRONG`: the docs say something false
- `MINOR`

The decisions table is:

| # | Decision | Commit or reason |
|---|---|---|

Decision is one of:

- `fixed`
- `documented`
- `won't fix`
- `open`

Every finding gets a line, and `won't fix` always gives the reason. `audit/README.md` lists the reports with their date, commit and score.

## When

- Before each minor release (`0.x.0`): all three campaigns.
- Patch release: no campaign, unless it changes the public API or the docs; then only the steps it touches are rerun.
- A release ships with no open `BLOCKER` or `DOC-WRONG` finding.

## Limits

- The testers are AI agents playing users, not real users. They read docs faster and more literally than people, and they share the blind spots of their model family.
- A blind test measures usability, not performance or security. Those have their own checks: [How Rango is built and verified](./how-rango-is-built.md) and [Security requirements](./security-requirements.md).
- Reports are kept as written, including findings that were later judged wrong.
