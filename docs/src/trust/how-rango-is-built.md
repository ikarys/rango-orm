# How Rango is built and verified

## How the code is written

Rango is written with AI assistance: AI coding agents write most of the code. The maintainer sets the requirements below, reviews every change and makes the decisions. An AI-written change goes through exactly the same checks as a hand-written one.

So you do not have to trust the process: every check on this page can be re-run from a clone of the repository.

## Requirements

Each requirement names what enforces it. "Review" means the maintainer checks it by hand; everything else fails a build or a test.

### Architecture
- Every database operation is async; nothing blocks the runtime. Enforced by: the public API (every operation returns a future), review.
- The core crate (`rango-orm-core`) has no database dependency; each backend is its own crate. Enforced by: the crate manifests (`cargo tree -p rango-orm-core` shows no sqlx).
- SQL generation is shared, not copied per backend: one query builder in core, one `Dialect` per database. Enforced by: review, unit tests on the generated SQL.
- Layers stay separate: schema types and SQL generation without IO, execution in the backends, the CLI on top. Enforced by: crate boundaries.

### Security

The full list, with the test behind each claim and what Rango does *not* protect, is in [Security requirements](./security-requirements.md). In short:
- Values are always bound parameters, never interpolated into SQL.
- Identifiers are always quoted through one function, `quote_ident`.
- Operators and connectors are Rust enums, never strings from input.
- `FieldPassword` stores an Argon2id hash and never prints or serializes the secret.
- Remote PostgreSQL and MySQL connections require TLS unless the URL says otherwise.
- `#![forbid(unsafe_code)]` in every crate; `cargo audit` and `cargo deny` in CI.

### Performance
- No N+1 by construction: `select_related` is one JOIN, `prefetch_related` one query per relation. Enforced by: integration tests, query counts in the blind tests.
- `RETURNING` instead of a re-fetch where the database supports it; bulk operations are one statement per chunk. Enforced by: integration tests, benchmarks.
- Every foreign key and many-to-many pivot is indexed. Enforced by: migration tests on the generated SQL.
- No regression goes unnoticed: benchmarks are compared A/B with the previous version before a release, and against sqlx, SeaORM and Diesel ([benchmarks/README.md](https://github.com/ikarys/rango-orm/blob/main/benchmarks/README.md)).

### API and style
- Errors are typed (`rango::Error`), never strings, and they carry no SQL text. Enforced by: the type signatures (`anyhow` only in the CLI binary).
- A wrong column or a wrong value type is a compile error with typed columns. Enforced by: `compile_fail` doctests.
- Formatting and lints: `cargo fmt --check`, `cargo clippy -D warnings`; `cargo doc` with warnings as errors.
- Minimal dependencies: every dependency must earn its place. Enforced by: review, `cargo deny`.

## What CI runs on every push

| Job | What it checks |
|---|---|
| Check & Lint | `cargo fmt --check`, `cargo clippy -D warnings` |
| Package, docs & MSRV | `cargo doc` with warnings as errors, `cargo publish --dry-run`, `cargo check` on MSRV Rust 1.94 |
| Security audit | `cargo audit`, `cargo-deny` for licenses, sources, and advisories |
| Unit tests | unit tests and the SQLite integration suite (in memory) |
| Integration tests (PostgreSQL 14, 18) | integration and CLI migration tests on PostgreSQL 14 and 18 |
| Integration tests (MySQL / MariaDB) | integration and CLI migration tests on MySQL 8.0, 8.4 and MariaDB 10.6, 11.4 |

A change is merged only when every job passes.

## Before each release
- Run the full integration suite on PostgreSQL, SQLite, MySQL, and MariaDB.
- Run A/B benchmarks against the previous release; the method is in `benchmarks/README.md`.
- Run a blind usability test: an agent reads only the user docs, builds a real app, and logs every friction; the protocol and reports are in the `audit/` directory of the repository.
- Run a security review against the checklist in [Security requirements](./security-requirements.md).
- Fix findings or record a decision.

## What this does not prove
- Rango is pre-1.0 and young.
- No external security audit has been done yet.
- No large production deployments have been reported yet.
- The blind usability tests are run by AI agents playing users, not by real users.
- The benchmarks are run on one machine.

## Re-run it yourself

```sh
just check # fmt + clippy
just test # unit
just test-sqlite # SQLite tests
just test-postgres # PostgreSQL tests
just test-mysql # MySQL 8.4 + MariaDB 11.4 in Docker
just test-all # all test suites
just scenarios # end-to-end blog/CMS app on SQLite
just bench-sqlite # SQLite benchmark
just bench-postgres # PostgreSQL benchmark
just bench-compare # Rango vs sqlx raw, SeaORM, Diesel
just audit # dependency audit
just deny # dependency policy check
```