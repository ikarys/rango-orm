# MySQL / MariaDB

One backend, `rango-mysql`, covers MySQL 8.0+ and MariaDB 10.6+. It sticks to the subset both servers share, so the same models, queries and migration files work on either.

## Setup

```toml
# Cargo.toml
[dependencies]
rango = { git = "https://github.com/ikarys/rango-orm", default-features = false, features = ["mysql"] }
```

```toml
# rango.toml
[database]
backend = "mysql"   # "mariadb" is accepted too
```

```bash
export DATABASE_URL=mysql://user:password@localhost:3306/mydb
# mariadb:// is accepted as an alias of mysql://
```

## Connecting

```rust
use rango::prelude::*;

let pool = connect_mysql(&std::env::var("DATABASE_URL")?).await?;
```

Every connection uses `utf8mb4`, runs in UTC (`DATETIME` has no time zone) and in strict SQL mode, so invalid values are rejected instead of being silently truncated.

## Type mapping

| Rango type | MySQL / MariaDB column |
|---|---|
| `FieldUuid` | `BINARY(16)` |
| `FieldBool` | `BOOLEAN` (`TINYINT(1)`) |
| `FieldInt` / `FieldBigInt` / `FieldSmallInt` | `INT` / `BIGINT` / `SMALLINT` |
| `FieldAutoInt` / `FieldAutoBigInt` | `INT` / `BIGINT AUTO_INCREMENT` (id read back with `LAST_INSERT_ID()`) |
| `FieldDecimal<P, S>` | `DECIMAL(P, S)` |
| `FieldVarchar<_, N>` | `VARCHAR(N)` |
| `FieldText` | `LONGTEXT` |
| `FieldBytes` | `LONGBLOB` |
| `FieldJson` | `JSON` (a `LONGTEXT` with a `json_valid` check on MariaDB) |
| `FieldDate` / `FieldTime` / `FieldDateTime` | `DATE` / `TIME(6)` / `DATETIME(6)` |

Tables are created with `ENGINE=InnoDB DEFAULT CHARSET=utf8mb4`.

## Differences from Postgres

| Feature | Postgres | MySQL / MariaDB |
|---|---|---|
| `RETURNING *` | ✅ | ❌: `insert`/`update` return the model you passed (every column is bound from it) |
| Transactional DDL | ✅ | ❌: a migration that fails halfway leaves its earlier statements applied |
| Partial unique constraint (`.condition(...)`) | `CREATE UNIQUE INDEX … WHERE` | skipped with a comment in the migration; enforce it in application code |
| `get_or_create` | `INSERT … ON CONFLICT DO NOTHING RETURNING *` | plain `INSERT`, then a `SELECT` on unique violation |
| Sub-microsecond timestamps | ✅ | truncated to microseconds |

## Migrations

`rango makemigrations` writes backtick-quoted SQL that also runs as-is in the `mysql` / `mariadb` client. Unique constraints, indexes and foreign keys are named (`{table}_{col}_unique`, `{table}_{col}_idx`, `{table}_{col}_fkey`) so later migrations can drop them. Type and nullability changes use `MODIFY COLUMN`.

MySQL caps identifiers at 64 characters. Constraint and index names that would exceed it are cut and suffixed with a stable hash. Table names are not shortened: if a table or M2M pivot name (`{prefix}_{model}_{prefix}_{other}`) is too long, `makemigrations` stops with an error. Use a shorter `-p` prefix.

`rango migrate` takes a `GET_LOCK('rango_migrate')` advisory lock, so two deploys cannot apply migrations at the same time.

## Export / import

`rango export` writes `BINARY(16)` values as UUID strings and JSON columns as JSON (MariaDB included). `rango import` runs in one transaction with foreign key checks off for that session, so tables can appear in any order and a failed import leaves nothing behind. It converts UUID strings back to 16 bytes. Rows that already exist are skipped, or overwritten with `--replace`.

## Local testing

```bash
just db-mysql     # MySQL 8.4 on :3307, MariaDB 11.4 on :3308 (Docker)
just test-mysql   # backend + CLI e2e tests against both
```
