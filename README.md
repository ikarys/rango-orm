# Rango ORM

> A Django-inspired ORM for Rust — define structs, not schemas. Change your model, run one command, migrations write themselves.

[![CI](https://github.com/ikarys/rango-orm/actions/workflows/ci.yml/badge.svg)](https://github.com/ikarys/rango-orm/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/ikarys/rango-orm/graph/badge.svg)](https://codecov.io/gh/ikarys/rango-orm)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

🚧 **Work in progress** — not ready for production use.

---

## Philosophy

- **Models first** — define a Rust struct, Rango infers the database schema
- **Zero ceremony** — no SQL migrations to write by hand, no boilerplate
- **Convention over configuration** — sensible defaults, override only when needed
- **100% async** — built on [sqlx](https://github.com/launchbadge/sqlx) and [tokio](https://tokio.rs)

## Quick look

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
rango makemigrations   # generates SQL migrations from your structs
rango migrate          # applies pending migrations
```

## Status

| Feature | Status |
|---|---|
| Models + derive macro | ✅ |
| Auto migrations (`makemigrations` / `migrate`) | ✅ |
| Query builder | ✅ |
| Relations (FK, M2M, select_related, prefetch_related) | ✅ |
| Transactions | ✅ |
| Bulk ops | ✅ |
| Aggregations | ✅ |
| Raw SQL escape hatch | ✅ |
| PostgreSQL backend | 🚧 |
| MySQL / SQLite | 📋 planned |

## License

MIT
