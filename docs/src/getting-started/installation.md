# Installation

## Add to your project

Rango is in beta: pin the version, the API may still change between betas. The package is `rango-orm`; the crate you import is still `rango`.

```toml
# Cargo.toml

[dependencies]
# PostgreSQL backend (default)
rango-orm = "0.1.0-beta.3"

# SQLite backend
rango-orm = { version = "0.1.0-beta.3", default-features = false, features = ["sqlite"] }

# MySQL 8.0+ / MariaDB 10.6+ backend
rango-orm = { version = "0.1.0-beta.3", default-features = false, features = ["mysql"] }

tokio = { version = "1", features = ["full"] }
```

## Install the CLI

```bash
cargo install rango-orm-cli --version 0.1.0-beta.3
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

- Rust 1.85+ (edition 2024)
- For PostgreSQL: a running Postgres instance
- For MySQL / MariaDB: a running MySQL 8.0+ or MariaDB 10.6+ instance
- For SQLite: nothing extra needed
