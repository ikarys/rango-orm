# Rango ORM

> A Django-inspired ORM for Rust. The goal is simple: abstract away the complexity of database queries and deliver a smooth, concise, and enjoyable experience — without sacrificing performance or security.

## Philosophy

Rango is built on a simple idea: **you define your data, Rango handles the rest**.

No SQL to write by hand. No migration files to maintain. No boilerplate.

```rust
use rango::prelude::*;

#[derive(Model)]
struct Post {
    id: FieldUuid,
    title: FieldVarchar<1, 255>,
    body: FieldText,
    published: FieldBool,
    created_at: FieldDateTime,
}
```

```bash
rango makemigrations   # generates SQL from your structs
rango migrate          # applies pending migrations
```

That's it.

## Key Features

- **Auto-migrations** — diff your structs, generate SQL, never write migrations by hand
- **Type-safe query builder** — filter, order, paginate with full compile-time safety
- **Relations** — `ForeignKey`, `ManyToMany`, `select_related`, `prefetch_related`
- **Multiple backends** — PostgreSQL and SQLite, same API
- **Async-first** — built on [sqlx](https://github.com/launchbadge/sqlx) and [tokio](https://tokio.rs)
- **CLI tools** — `init`, `makemigrations`, `migrate`, `export`, `import`

## How Rango compares

Rango targets Django-style ORM ergonomics on native async SQLx.

| | **Rango** | Diesel | SeaORM | SQLx | Butane |
|---|---|---|---|---|---|
| Kind | Django-style ORM | Query builder + ORM | Async ORM | Raw SQL toolkit (not an ORM) | ORM |
| Migrations | ✅ auto-generated from models (`rango makemigrations`) | ⚠️ Diesel CLI; SQL by hand | ⚠️ Rust files by hand; schema sync newer | ⚠️ sqlx-cli; SQL by hand | ✅ auto-generated from models |
| Model definition | `#[derive(Model)]` only | `table!` macro + struct | Entity + ActiveModel, more boilerplate | None | `#[model]` struct |
| Async | ✅ native (sqlx) | ⚠️ via `diesel-async` | ✅ | ✅ | ⚠️ partial |
| Compile-time column checks | ✅ generated typed columns (`User::EMAIL`): name and value type checked; string names still accepted | ✅ strong | ❌ runtime | ✅ `query!` macros, needs a dev DB | ✅ |
| Model hooks | ✅ `ModelHooks` | ❌ | ✅ ActiveModelBehavior | ❌ | ❌ |
| Relations loading | ✅ `select_related` / `prefetch_related` | Manual joins | ✅ | ❌ | ✅ |
| Real `ON DELETE CASCADE` | ✅ | ✅ | ✅ | n/a | ✅ |
| Raw SQL escape hatch | ✅ `raw*` | ✅ | ✅ | ✅ it's the point | ✅ |
| Backends | PG, MySQL / MariaDB, SQLite | PG, MySQL, SQLite | PG, MySQL, SQLite | PG, MySQL, SQLite | PG, SQLite |
| Maturity / ecosystem | Early, API not stable | Large, production at crates.io | Medium, active | Large | Small |

### Where Rango is weaker today

- Bulk reads are ~6% slower than hand-written sqlx on 1000-row selects (on par with SeaORM, Diesel is ~7% faster) — see benchmarks.
- Young project with a small ecosystem.

Comparison inspired by Shuttle's [Best Rust ORMs](https://www.shuttle.dev/blog/2024/01/16/best-orm-rust) (2024), extended with Butane and Rango.

## Status

Rango is under active development. The API is not yet stable.
PostgreSQL, SQLite and MySQL / MariaDB backends are functional.

## License

MIT
