# Rango ORM

A Django-inspired async ORM for Rust: define a struct, Rango generates the schema, the migrations and a typed query API.

[![CI](https://github.com/ikarys/rango-orm/actions/workflows/ci.yml/badge.svg)](https://github.com/ikarys/rango-orm/actions/workflows/ci.yml) [![codecov](https://codecov.io/gh/ikarys/rango-orm/graph/badge.svg)](https://codecov.io/gh/ikarys/rango-orm)

> Work in progress: API is not stable, and Rango is not on crates.io yet.

## Why Rango

- Models first: the struct is the schema.
- Migrations generated, never written: `rango makemigrations` diffs typed schema snapshots against the models.
- Typed queries: columns are generated per field; a wrong column or wrong value type is a compile error, and string names are still accepted.
- Real SQL semantics: `ON DELETE CASCADE` in the DB, every FK indexed, `RETURNING *`.
- 100% async on sqlx, with PostgreSQL and SQLite using the same API.
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
rango::atomic(&pool, |tx| Box::pin(async move {
    rango::insert(&mut *tx, post).await?;
    Ok(())
})).await?;
```

```bash
rango init --backend postgres   # creates rango.toml
rango makemigrations            # writes migrations/0001_….sql from your structs
rango migrate                   # applies pending migrations (reads DATABASE_URL)
```

## Installation

```toml
[dependencies]
rango = { git = "https://github.com/ikarys/rango-orm" } # PostgreSQL, default
# SQLite variant:
# rango = { git = "https://github.com/ikarys/rango-orm", default-features = false, features = ["sqlite"] }
tokio = { version = "1", features = ["full"] }
```

```bash
cargo install --git https://github.com/ikarys/rango-orm rango-cli
```

In production, pass the URL via `DATABASE_URL`; never put credentials in `rango.toml`.

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
| SQLite backend | ✅ (ALTER via table rebuild 🚧) |
| MySQL / MariaDB | 📋 planned |
| Keyset pagination, optimistic locking | 📋 planned |

## Rango vs other Rust ORMs

|  | Diesel | SeaORM | sqlx | **Rango** |
|---|---|---|---|---|
| Migrations | SQL by hand | Rust files by hand | SQL by hand | generated from structs |
| Model definition | `table!` macro + struct | Entity + ActiveModel | none | `#[derive(Model)]` only |
| Async | via diesel-async | yes | yes | yes, native |
| Typed queries | strong | runtime column checks | `query!` macros need a live DB | typed columns, name and value checked |
| Maturity | large | medium | large | early |

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
| `rango` | facade crate, re-exports everything, features `postgres` default / `sqlite` |
| `rango-core` | DB-free traits, schema types, field types |
| `rango-derive` | `#[derive(Model)]` |
| `rango-postgres` | PostgreSQL backend (sqlx) |
| `rango-sqlite` | SQLite backend (sqlx) |
| `rango-cli` | `rango` binary |
| `rango-tests` | integration tests |
| `benchmarks/` | Criterion: Rango vs sqlx, SeaORM, Diesel |

## Development

```bash
just dev          # starts Postgres in Docker
just test-all     # unit + SQLite + Postgres integration tests
just check        # fmt + clippy
just bench-compare
just audit
```

## License

MIT, see [LICENSE](LICENSE).