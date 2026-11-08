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
sqlx logs one event per statement on target `sqlx::query`, at `DEBUG` level. Fields:
- `summary`: the SQL text when it is short, otherwise a truncated form ending in `…`
- `db.statement`: the full SQL text, filled only when `summary` is truncated; an empty string for short statements (their SQL is in `summary`)
- `rows_affected`
- `rows_returned`
- `elapsed`

Bound values are never logged.

Statements slower than the threshold are logged on the same target at `WARN` ("slow statement"), with the same fields plus `slow_threshold`.

## Slow query log
- Postgres: `DatabaseConfig::slow_query_threshold_ms` (default `500`) logs statements slower than the threshold at `WARN`.
- SQLite and MySQL: fixed `500` ms.

## Setup
The per-statement events are `DEBUG`, so a default `info` filter hides them. To see every statement, enable `sqlx::query=debug`; to keep only the slow-statement warnings, use `sqlx::query=warn` (it hides all the other statements).

```rust,ignore
tracing_subscriber::fmt()
    .with_env_filter("info,sqlx::query=debug")   // every statement
    // .with_env_filter("info,sqlx::query=warn") // slow statements only
    .init();
```

## OpenTelemetry
To export to OpenTelemetry, add `tracing-opentelemetry` to your app. Rango's span fields already follow the conventions. This is the app's choice; Rango does not depend on OpenTelemetry.
