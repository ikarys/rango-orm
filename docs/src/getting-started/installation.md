# Installation

## Add to your project

```toml
# Cargo.toml

[dependencies]
# SQLite backend (default)
rango = { git = "https://github.com/ikarys/rango-orm", features = ["sqlite"] }

# PostgreSQL backend
rango = { git = "https://github.com/ikarys/rango-orm", features = ["postgres"] }
```

> **Note:** Rango is not yet published on crates.io.

## Install the CLI

```bash
cargo install --git https://github.com/ikarys/rango-orm rango
```

## Initialize a project

```bash
rango init              # SQLite by default
rango init --backend postgres
```

This creates:
```
rango.toml        ← configuration
migrations/       ← SQL migration files (generated)
```

## Requirements

- Rust 1.75+
- For PostgreSQL: a running Postgres instance
- For SQLite: nothing extra needed
