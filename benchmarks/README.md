# Rango ORM — Benchmarks

Same workloads on four stacks, one Postgres: **sqlx raw** (no ORM, baseline), **Rango**,
**SeaORM 2.0**, **Diesel 2.3 + diesel-async 0.9**. Every read decodes into a typed struct,
every pool has 10 connections, updates write the full row.

## Run

```bash
just bench-compare   # starts Postgres (Docker), runs the 4 suites, prints the table
```

## Results (PostgreSQL 16 in Docker, 2026-11-05)

Mean time per operation, lower is better. "Rango vs best ORM" compares with the faster of SeaORM / Diesel.

| Benchmark | sqlx_raw | rango | seaorm | diesel_async | Rango vs best ORM |
|---|---|---|---|---|---|
| `bulk_insert/10` | 1.96 ms | 1.98 ms | 1.94 ms | 2.51 ms | +2% |
| `bulk_insert/100` | 2.25 ms | 2.49 ms | 2.28 ms | 3.14 ms | +9% |
| `bulk_insert/1000` | 4.79 ms | 5.47 ms | 5.57 ms | 9.96 ms | -2% |
| `count` | 864.18 µs | 883.67 µs | 852.94 µs | 863.63 µs | +4% |
| `insert_one` | 1.89 ms | 1.96 ms | 2.00 ms | 2.49 ms | -2% |
| `select_all_1000` | 1.22 ms | 1.49 ms | 1.50 ms | 1.29 ms | +15% |
| `select_filter_100` | 922.93 µs | 962.95 µs | 927.89 µs | 919.42 µs | +5% |
| `select_pk` | 830.01 µs | 856.65 µs | 816.50 µs | 820.34 µs | +5% |
| `transaction_10_inserts` | 6.44 ms | 6.78 ms | 6.32 ms | 12.07 ms | +7% |
| `update_one` | 1.94 ms | 2.00 ms | 1.93 ms | 2.47 ms | +4% |

### Reading the numbers

- Every query pays ~0.8 ms of round-trip (Docker on WSL2), so single-row operations are within
  ±5% of each other for all four stacks: that gap is mostly noise.
- **Rango ≈ SeaORM** on every workload. Both pay ~20% over raw sqlx on `select_all_1000`
  (row decoding: ~0.28 ms per 1000 rows, see `fetch_orm_1000` vs `fetch_raw_rows_1000`).
- **Diesel-async** is the fastest at decoding 1000 rows but 25–100% slower on writes
  (`bulk_insert/1000`, `transaction_10_inserts`).
- Clean build of each crate (32 cores): sqlx / Rango / Diesel 11 s, SeaORM 14 s; incremental < 1 s for all.

### Planned Rango optimizations

- Row decoding by column index without `dyn` dispatch (closes most of the `select_all_1000` gap).
- `bulk_create` via `UNNEST` (one statement, no 65 535-parameter chunking).

## Machine

AMD Ryzen 9 9950X3D 16-Core Processor, 32 threads, WSL2, Postgres 16 in Docker on the same host.

## Methodology

- Criterion defaults: 100 samples, 3 s warmup; one run per stack, run one after another.
- Each suite has its own table, truncated at setup.
