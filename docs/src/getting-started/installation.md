# Installation

## Add to your project

Rango is in beta and not on crates.io yet: depend on a git tag, the API may still change between betas.

```toml
# Cargo.toml

[dependencies]
# PostgreSQL backend (default)
rango = { git = "https://github.com/ikarys/rango-orm", tag = "v0.1.0-beta.2" }

# SQLite backend
rango = { git = "https://github.com/ikarys/rango-orm", tag = "v0.1.0-beta.2", default-features = false, features = ["sqlite"] }

# MySQL 8.0+ / MariaDB 10.6+ backend
rango = { git = "https://github.com/ikarys/rango-orm", tag = "v0.1.0-beta.2", default-features = false, features = ["mysql"] }

tokio = { version = "1", features = ["full"] }
```

## Install the CLI

```bash
cargo install --git https://github.com/ikarys/rango-orm --tag v0.1.0-beta.2 rango-cli
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
