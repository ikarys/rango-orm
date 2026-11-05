# Field Types

All field types live in `rango::prelude::*`.

## Primitive types

| Rust type | SQL (Postgres) | SQL (SQLite) | Notes |
|---|---|---|---|
| `FieldBool` | `BOOLEAN` | `INTEGER` | |
| `FieldSmallInt` | `SMALLINT` | `INTEGER` | |
| `FieldInt` | `INTEGER` | `INTEGER` | |
| `FieldBigInt` | `BIGINT` | `INTEGER` | |
| `FieldFloat` | `REAL` | `REAL` | |
| `FieldDouble` | `DOUBLE PRECISION` | `REAL` | |

## Text types

| Rust type | SQL (Postgres) | SQL (SQLite) | Notes |
|---|---|---|---|
| `FieldText` | `TEXT` | `TEXT` | Unlimited length |
| `FieldVarchar<MIN, MAX>` | `VARCHAR(MAX)` | `TEXT` | Compile-time length bounds |
| `FieldEmail` | `VARCHAR(254)` | `TEXT` | |
| `FieldUrl` | `VARCHAR(2048)` | `TEXT` | |
| `FieldPassword<MIN, MAX>` | `VARCHAR(MAX)` | `TEXT` | |

## Numeric types

| Rust type | SQL (Postgres) | SQL (SQLite) | Notes |
|---|---|---|---|
| `FieldDecimal<P, S>` | `NUMERIC(P, S)` | `REAL` | P = precision, S = scale |
| `FieldRange<MIN, MAX>` | `BIGINT` | `INTEGER` | Compile-time range check |

## Date/time types

| Rust type | SQL (Postgres) | SQL (SQLite) |
|---|---|---|
| `FieldDateTime` | `TIMESTAMPTZ` | `TEXT` (ISO 8601) |
| `FieldDate` | `DATE` | `TEXT` |
| `FieldTime` | `TIME` | `TEXT` |

## Other types

| Rust type | SQL (Postgres) | SQL (SQLite) |
|---|---|---|
| `FieldUuid` | `UUID` | `TEXT` |
| `FieldJson` | `JSONB` | `TEXT` |
| `FieldBytes` | `BYTEA` | `BLOB` |

## Relation types

| Rust type | Description |
|---|---|
| `ForeignKey<M>` | Foreign key to model `M` — stored as UUID |
| `ManyToMany<M>` | Many-to-many relation — pivot table auto-generated |

## Nullable

Wrap any field type in `Option<T>` to make it nullable:

```rust
pub score: Option<FieldInt>,     // INTEGER NULL
pub bio: Option<FieldText>,      // TEXT NULL
pub avatar: Option<FieldUuid>,   // UUID NULL
```
