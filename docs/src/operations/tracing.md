# Tracing
Every database operation (CRUD, query builder executions, aggregates, bulk ops, many-to-many ops, raw SQL) emits a `tracing` span named `db.query`.

## Span fields
| Field | Value |
|---|---|
| `db.system` | `postgresql`, `sqlite`, or `mysql` |
| `db.operation` | SQL operation: `SELECT`, `INSERT`, `UPDATE`, `DELETE`, ... |
| `db.sql.table` | Model table (absent on raw SQL) |
| `db.rows` | Rows returned, filled after the query |
| `otel.kind` | `"client"` |

## sqlx events
sqlx additionally logs one event per statement on target `sqlx::query`:
- `db.statement`
- `rows_affected`
- `rows_returned`
- `elapsed`

Bound values are never logged.

## Slow query log
- Postgres: `DatabaseConfig::slow_query_threshold_ms` (default `500`) logs statements slower than the threshold at `WARN`.
- SQLite: fixed `500` ms.

## Setup
```rust,ignore
tracing_subscriber::fmt()
    .with_env_filter("info,sqlx::query=warn")
    .init();
```

## OpenTelemetry
To export to OpenTelemetry, add `tracing-opentelemetry` to your app. Rango's span fields already follow the conventions. This is the app's choice; Rango does not depend on OpenTelemetry.
