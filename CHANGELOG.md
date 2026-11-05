# Changelog

## 0.1.0-beta.1 (2026-11-05)

First beta. The API may still change between betas: pin the tag.

- `#[derive(Model)]` with typed field types, constraints, ordering and FKs with real `ON DELETE`.
- PostgreSQL and SQLite backends: CRUD, bulk create/update/upsert, transactions (`atomic`, `atomic_retry` on Postgres), raw SQL.
- Typed query builder: `Column<M, T>` constants, so comparing a column to a value of the wrong type does not compile.
- Typed errors (`rango::Error`), TLS by default for remote Postgres, argon2id `FieldPassword`.
- `rango` CLI: `init`, `makemigrations`, `migrate`, `export`, `import`.
