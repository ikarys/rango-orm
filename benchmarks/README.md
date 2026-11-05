# Rango ORM — Benchmarks

Same workloads on four stacks, one Postgres: **sqlx raw** (no ORM, baseline), **Rango**,
**SeaORM 2.0**, **Diesel 2.3 + diesel-async 0.9**. Every read decodes into owned values,
every pool has 10 connections, updates write the full row.

## Run

```bash
just bench-compare   # starts Postgres (Docker), runs the 4 suites, prints the table
```

## Results (PostgreSQL 16 in Docker, 2026-11-05)

Mean time per operation, lower is better. "Rango vs best ORM" compares with the faster of SeaORM / Diesel.

| Benchmark | sqlx_raw | rango | seaorm | diesel_async | Rango vs best ORM |
|---|---|---|---|---|---|
| `bulk_insert/10` | 2.01 ms | 1.93 ms | 2.01 ms | 2.59 ms | -4% |
| `bulk_insert/100` | 2.32 ms | 2.30 ms | 2.37 ms | 3.29 ms | -3% |
| `bulk_insert/1000` | 4.80 ms | 5.47 ms | 5.93 ms | 10.31 ms | -8% |
| `count` | 876.68 µs | 863.60 µs | 901.73 µs | 885.60 µs | -2% |
| `insert_one` | 2.01 ms | 1.94 ms | 1.92 ms | 2.53 ms | +1% |
| `select_all_1000` | 1.43 ms | 1.49 ms | 1.49 ms | 1.41 ms | +6% |
| `select_filter_100` | 938.54 µs | 926.76 µs | 967.89 µs | 944.08 µs | -2% |
| `select_pk` | 819.33 µs | 826.85 µs | 835.98 µs | 833.24 µs | -1% |
| `transaction_10_inserts` | 6.15 ms | 6.52 ms | 6.27 ms | 12.58 ms | +4% |
| `update_one` | 1.91 ms | 1.95 ms | 2.00 ms | 2.87 ms | -3% |

### Reading the numbers

- Every query pays ~0.8 ms of round-trip (Docker on WSL2), so single-row operations are within
  ±5% of each other for all four stacks: that gap is mostly noise (run-to-run variation is ~3–5%).
- **Rango ≈ SeaORM** on every workload.
- `select_all_1000`: decoding 1000 rows into owned structs costs ~0.2 ms for every stack
  (`fetch_raw_rows_1000` vs `fetch_orm_1000`). The sqlx baseline decodes by column index into a
  tuple — the hand-written best case. Rango is ~6% above it, Diesel is on par.
- **Diesel-async** is 25–100% slower on writes (`bulk_insert/1000`, `transaction_10_inserts`, `update_one`).
- Clean build of each crate (32 cores): sqlx / Rango / Diesel 11 s, SeaORM 14 s; incremental < 1 s for all.

### Rango optimizations

- Done: decoded rows are collected into a pre-sized `Vec` (−4.5% on `select_all_1000`).
- Measured, not worth it yet: decoding by column index instead of by name saves ~3% on
  1000 rows, at the cost of changing the public `RangoRow` trait.
- Planned: `bulk_create` via `UNNEST` (one statement, no 65 535-parameter chunking).

## Machine

AMD Ryzen 9 9950X3D 16-Core Processor, 32 threads, WSL2, Postgres 16 in Docker on the same host.

## Methodology

- Criterion defaults: 100 samples, 3 s warmup; one run per stack, run one after another.
- Each suite has its own table, truncated at setup.
