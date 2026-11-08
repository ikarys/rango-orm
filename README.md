# Rango ORM

A Django-inspired async ORM for Rust: define a struct, Rango generates the schema, the migrations and a typed query API.

[![CI](https://github.com/ikarys/rango-orm/actions/workflows/ci.yml/badge.svg)](https://github.com/ikarys/rango-orm/actions/workflows/ci.yml) [![codecov](https://codecov.io/gh/ikarys/rango-orm/graph/badge.svg)](https://codecov.io/gh/ikarys/rango-orm)

> `0.1.1` is the latest release on crates.io as [`rango-orm`](https://crates.io/crates/rango-orm); this tree is `0.2.0-beta.1`, and the snippets below show the `0.2` form (use `0.2.0-beta.1` until it is released). Pre-1.0: a `0.x` minor release may still break the API.

## Why Rango

- Models first: the struct is the schema.
- Migrations generated, never written: `rango makemigrations` diffs typed schema snapshots against the models.
- Typed queries: columns are generated per field; a wrong column or wrong value type is a compile error, and string names are still accepted.
- Real SQL semantics: `ON DELETE CASCADE` in the DB, every FK indexed, `RETURNING *` where the database has it.
- 100% async on sqlx, with PostgreSQL, SQLite and MySQL / MariaDB using the same API.
- Secure by default: bound parameters everywhere, identifiers always quoted, passwords hashed with Argon2id, TLS required for remote Postgres.

## Quick look

```rust
use rango::prelude::*;

#[derive(Model)]
struct User {
    id: FieldUuid,
    #[field(unique)]
    email: FieldEmail,
    name: FieldVarchar<1, 100>,
    age: FieldInt,
    active: FieldBool,
    created_at: FieldDateTime,
}

#[derive(Model)]
struct Post {
    id: FieldUuid,
    author_id: ForeignKey<User>, // REFERENCES "user" ON DELETE CASCADE, indexed
    title: FieldVarchar<1, 255>,
    published: FieldBool,
}
```

```rust
let adults = User::filter(&pool)
    .gte(User::AGE, 18)
    .eq(User::ACTIVE, true)
    .order_by(User::CREATED_AT.desc())
    .limit(20)
    .all()
    .await?;

// Does not compile: `&str` cannot be compared to a `FieldInt` column
// User::filter(&pool).eq(User::AGE, "eighteen");

let user: Option<User> = rango::get(&pool, &SqlValue::from(id)).await?;
rango::atomic(&pool, async |tx| {
    rango::insert(&mut **tx, post).await?;
    Ok(())
}).await?;
```

```bash
rango init --backend postgres   # creates rango.toml
rango makemigrations            # writes migrations/0001_….sql from your structs
rango migrate                   # applies pending migrations (reads DATABASE_URL)
```

## Installation

Rango is pre-1.0: a `0.x` minor release may still break the API. The package is `rango-orm`; the crate you import is `rango` (`use rango::prelude::*`).

```toml
[dependencies]
rango-orm = "0.2" # PostgreSQL, default
# SQLite variant:
# rango-orm = { version = "0.2", default-features = false, features = ["sqlite"] }
# MySQL / MariaDB variant:
# rango-orm = { version = "0.2", default-features = false, features = ["mysql"] }
tokio = { version = "1", features = ["full"] }
```

```bash
cargo install rango-orm-cli --version 0.2.0-beta.1
```

In production, pass the URL via `DATABASE_URL`; never put credentials in `rango.toml`.

### Supported databases

| Database | Supported | Tested in CI | Why this minimum |
|---|---|---|---|
| PostgreSQL | 14+ | 14, 18 | `gen_random_uuid()` is built in from 13; 13 is end-of-life |
| MySQL | 8.0+ | 8.0, 8.4 | oldest version tested |
| MariaDB | 10.6+ | 10.6, 11.4 | oldest version tested |
| SQLite | 3.35+ | bundled 3.46 | `RETURNING` (3.35), `UPDATE … FROM` (3.33); sqlx bundles SQLite, so nothing to install |

## Features

| Feature | Status |
|---|---|
| Models + `#[derive(Model)]` | ✅ |
| Auto migrations (`makemigrations` / `migrate`, rename detection) | ✅ |
| Typed query builder (filter, `and`/`or`/`not`, order, limit/offset, typed columns) | ✅ |
| Relations: `ForeignKey`, `ManyToMany`, `select_related`, `prefetch_related` | ✅ |
| Constraints: check, unique, partial unique | ✅ |
| Transactions (`atomic`, `atomic_retry` on serialization failure) | ✅ |
| Bulk create / update / upsert | ✅ |
| Aggregations | ✅ |
| Model hooks (`ModelHooks`) | ✅ |
| Raw SQL escape hatch | ✅ |
| `export` / `import` CLI | ✅ |
| PostgreSQL backend | ✅ |
| SQLite backend | ✅ (ALTER via table rebuild) |
| MySQL 8.0+ / MariaDB 10.6+ backend | ✅ (no `RETURNING`, non-transactional DDL) |
| Keyset pagination, optimistic locking | 📋 planned |

## Rango vs other Rust ORMs

|  | **Rango** | Diesel | SeaORM | sqlx |
|---|---|---|---|---|
| Migrations | generated from structs | SQL by hand | Rust files by hand | SQL by hand |
| Model definition | `#[derive(Model)]` only | `table!` macro + struct | Entity + ActiveModel | none |
| Async | yes, native | via diesel-async | yes | yes |
| Typed queries | typed columns, name and value checked | strong | runtime column checks | `query!` macros need a live DB |
| Maturity | early | large | medium | large |

Full comparison: [docs](docs/src/introduction.md).

## Performance

On PostgreSQL, Rango is on par with SeaORM and Diesel and close to hand-written sqlx; mean per operation, lower is better.

| Benchmark | sqlx (raw) | Rango | SeaORM | Diesel-async |
|---|---|---|---|---|
| `select_pk` | 819 µs | 827 µs | 836 µs | 833 µs |
| `select_all_1000` | 1.43 ms | 1.49 ms | 1.49 ms | 1.41 ms |
| `insert_one` | 2.01 ms | 1.94 ms | 1.92 ms | 2.53 ms |
| `bulk_insert/1000` | 4.80 ms | 5.47 ms | 5.93 ms | 10.31 ms |
| `update_one` | 1.91 ms | 1.95 ms | 2.00 ms | 2.87 ms |
| `transaction_10_inserts` | 6.15 ms | 6.52 ms | 6.27 ms | 12.58 ms |

Postgres 16 in Docker on the same host; single-row differences under ~5% are noise. Methodology and full results: [benchmarks/README.md](benchmarks/README.md).

## Documentation

The user guide is an mdBook in `docs/` (`mdbook serve docs`).

- [Quick start](docs/src/getting-started/quick-start.md)
- [Defining models](docs/src/models/defining-models.md)
- [Filtering and typed columns](docs/src/querying/filter.md)
- [Generating migrations](docs/src/migrations/makemigrations.md)
- [Security model](docs/src/security/sql-injection.md)

## Workspace layout

| Crate | Role |
|---|---|
| `rango-orm` | facade crate (imported as `rango`), re-exports everything, features `postgres` default / `sqlite` / `mysql` |
| `rango-orm-core` | DB-free traits, schema types, field types |
| `rango-orm-derive` | `#[derive(Model)]` |
| `rango-orm-postgres` | PostgreSQL backend (sqlx) |
| `rango-orm-sqlite` | SQLite backend (sqlx) |
| `rango-orm-mysql` | MySQL / MariaDB backend (sqlx) |
| `rango-orm-cli` | `rango` binary |
| `rango-tests` | integration tests |
| `benchmarks/` | Criterion: Rango vs sqlx, SeaORM, Diesel |

## Development

```bash
just dev          # starts Postgres in Docker
just test-all     # unit + SQLite + Postgres + MySQL/MariaDB integration tests
just check        # fmt + clippy
just bench-compare
just audit
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.