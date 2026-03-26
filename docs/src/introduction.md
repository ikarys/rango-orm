# Rango ORM

> A Django-inspired ORM for Rust — define structs, not schemas. Change your model, run one command, migrations write themselves.

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

## Why Not Diesel / SeaORM?

| | Diesel | SeaORM | **Rango** |
|---|---|---|---|
| Write migrations | By hand | By hand | **Auto-generated** |
| Async | ❌ | ✅ | ✅ |
| Schema from code | ❌ | ❌ | ✅ |
| Django-like API | ❌ | Partial | ✅ |

## Status

Rango is under active development. The API is not yet stable.
PostgreSQL and SQLite backends are functional.

## License

MIT
