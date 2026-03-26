# Rango ORM — Benchmarks

Comparative benchmarks: Rango vs sqlx raw (zero ORM overhead baseline).

## Setup

```bash
# From repo root
docker compose up -d
```

## Run

```bash
cd benchmarks

# Rango
DATABASE_URL=postgres://postgres:postgres@localhost:5433/rango_test \
    cargo bench -p bench-rango

# sqlx raw baseline
DATABASE_URL=postgres://postgres:postgres@localhost:5433/rango_test \
    cargo bench -p bench-sqlx-raw
```

## Results (PostgreSQL 16, localhost)

| Benchmark | Rango | sqlx raw | Overhead |
|---|---|---|---|
| `insert_one` | 782 µs | 772 µs | **+1.3%** |
| `bulk_insert/10` | 829 µs (12k rows/s) | 863 µs (12k rows/s) | **-4%** ✅ |
| `bulk_insert/100` | 1.36 ms (74k rows/s) | 1.30 ms (77k rows/s) | **+4%** |
| `bulk_insert/1000` | 6.1 ms (163k rows/s) | 4.8 ms (206k rows/s) | **+21%** |
| `select_pk` | 83 µs | 82 µs | **+1.2%** |
| `count` | 87 µs | 107 µs | **-19%** ✅ |
| `update_one` | 799 µs | 764 µs | **+4.6%** |
| `select_all_1000` | 801 µs | 354 µs | **+126%** |

## Analysis

### Single-row operations
Overhead is **< 5%** on insert, update, select by PK. Dominated by network round-trip.

### Bulk operations
Rango's `bulk_create` uses a multi-row `INSERT ... VALUES (...)` — comparable to raw sqlx.
sqlx raw benchmark uses `UNNEST` which is faster for large batches (+21% at 1000 rows).

### select_all_1000
The 2x overhead comes from **deserialization** — mapping each `PgRow` to a typed Rust struct
via the `FromRow` trait (dynamic dispatch, ~0.46 µs/row on this machine).

Breaking it down:
- Network + DB execution: ~344 µs (raw fetch)
- ORM deserialization (`FromRow`): +457 µs
- Total: ~801 µs

**Planned optimization:** Direct `FromPgRow`/`FromSqliteRow` implementations (no dynamic dispatch)
should cut deserialization cost by ~2x. Zero breaking change for users.

## Machine

*Intel/AMD CPU — add your specs when running locally.*

## Methodology

- PostgreSQL 16 via Docker (local loopback — no network latency)
- Connection pool: default settings (20 max connections)
- criterion: 100 samples per benchmark
- Warmup: 3 seconds
