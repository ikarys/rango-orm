# Installation

## Add to your project

Rango is pre-1.0: a `0.x` minor release may still break the API. The package is `rango-orm`; the crate you import is still `rango`.

```toml
# Cargo.toml

[dependencies]
# PostgreSQL backend (default)
rango-orm = "0.2"

# SQLite backend
rango-orm = { version = "0.2", default-features = false, features = ["sqlite"] }

# MySQL 8.0+ / MariaDB 10.6+ backend
rango-orm = { version = "0.2", default-features = false, features = ["mysql"] }

tokio = { version = "1", features = ["full"] }

# Rango does not re-export these: add them yourself to build
# FieldUuid / FieldDateTime values (uuid::Uuid, chrono::Utc::now()).
uuid = { version = "1", features = ["v4"] }
chrono = "0.4"

# Needed for `FieldJson` values and for reading JSON columns from raw results.
serde_json = "1"
```

Rango 0.2 is in beta (`0.2.0-beta.1`). Until `0.2.0` is out, Cargo does not pick a pre-release for `"0.2"`: write `version = "0.2.0-beta.1"` in `Cargo.toml` (the snippets above show the final form).

### Several backends at once

You can enable more than one feature. `use rango::prelude::*` then holds the backend-agnostic items (models, field types, `Error`, ...), the `connect_*` functions and the pool types of every enabled backend, plus the ops (`insert`, `get`, `atomic`, `QueryBuilder`, ...) of **one** backend, chosen by priority: PostgreSQL if enabled, else SQLite, else MySQL.

The other backends' ops are always reachable by path: `rango::postgres::*`, `rango::sqlite::*`, `rango::mysql::*`.

```rust
use rango::prelude::*; // ops from Postgres (default feature is on)

rango::sqlite::insert(&sqlite_pool, item).await?;
// `Model::filter` comes from a per-backend trait; two of them in scope are ambiguous, so name it:
let q = <Item as rango::sqlite::RangoFilterExt>::filter(&sqlite_pool);
```

## Install the CLI

```bash
cargo install rango-orm-cli --version 0.2.0-beta.1
```

This installs the `rango` binary.

## Initialize a project

```bash
rango init                     # SQLite by default
rango init --backend postgres  # or mysql / mariadb
```

This creates:
```
rango.toml        ← configuration
migrations/       ← SQL migration files (generated)
```

## Requirements

- Rust 1.94+ (edition 2024)
- A database server for PostgreSQL / MySQL / MariaDB (SQLite needs nothing extra)

### Supported database versions

| Database | Supported | Tested in CI | Why this minimum |
|---|---|---|---|
| PostgreSQL | 14+ | 14, 18 | `gen_random_uuid()` is built in from 13; 13 is end-of-life |
| MySQL | 8.0+ | 8.0, 8.4 | oldest version tested |
| MariaDB | 10.6+ | 10.6, 11.4 | oldest version tested |
| SQLite | 3.35+ | bundled 3.46 | `RETURNING` (3.35), `UPDATE … FROM` (3.33); sqlx bundles SQLite, so nothing to install |
