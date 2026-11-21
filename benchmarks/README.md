# Rango ORM — Benchmarks

Same workloads on five stacks, one Postgres: **sqlx raw** (no ORM, baseline), **Rango**,
**SeaORM 2.0**, **Diesel 2.3 + diesel-async 0.9**, **Butane 0.8**. Every read decodes into owned values,
every pool has 10 connections, updates write the full row. Butane differs, see below.

## Run

```bash
just bench-compare   # starts Postgres (Docker), runs the 5 suites, prints the table
```

## Results (PostgreSQL 16 in Docker, 2026-11-21)

Mean time per operation, lower is better. "Rango vs best ORM" compares with the fastest of SeaORM / Diesel / Butane.

| Benchmark | sqlx_raw | rango | seaorm | diesel_async | butane | Rango vs best ORM |
|---|---|---|---|---|---|---|
| `bulk_insert/10` | 1.99 ms | 2.01 ms | 2.05 ms | 2.59 ms | 12.44 ms | -2% |
| `bulk_insert/100` | 2.37 ms | 2.34 ms | 2.45 ms | 3.10 ms | 100.01 ms | -4% |
| `bulk_insert/1000` | 5.01 ms | 4.71 ms | 5.94 ms | 9.39 ms | 988.06 ms | -21% |
| `count` | 905.32 µs | 879.80 µs | 909.31 µs | 920.63 µs | — | -3% |
| `insert_one` | 1.98 ms | 1.95 ms | 2.01 ms | 2.57 ms | 2.05 ms | -3% |
| `select_all_1000` | 1.30 ms | 1.46 ms | 1.42 ms | 1.29 ms | 1.51 ms | +13% |
| `select_filter_100` | 949.15 µs | 1.00 ms | 990.71 µs | 1.04 ms | 1.10 ms | +1% |
| `select_pk` | 853.14 µs | 879.49 µs | 887.51 µs | 955.21 µs | 1.03 ms | -1% |
| `transaction_10_inserts` | 6.58 ms | 6.69 ms | 6.89 ms | 12.32 ms | 12.31 ms | -3% |
| `update_one` | 1.96 ms | 2.00 ms | 2.00 ms | 2.60 ms | 2.06 ms | -0% |

### Reading the numbers

- Every query pays ~0.8 ms of round-trip (Docker on WSL2), so single-row operations are within
  a few percent of each other for sqlx, Rango and SeaORM: that gap is mostly noise (run-to-run variation is ~3–5%).
- **Rango ≈ SeaORM** on every workload.
- `select_all_1000`: decoding 1000 rows into owned structs costs ~0.2 ms for every stack
  (`fetch_raw_rows_1000` vs `fetch_orm_1000`). The sqlx baseline decodes by column index into a
  tuple — the hand-written best case. Rango is ~12% above it, Diesel is on par. On 2026-11-05 the gap was ~6%: Rango barely moved
  (1.49 → 1.44 ms) while sqlx and Diesel got ~10% faster (1.43 → 1.29 ms); two back-to-back reruns
  confirm the 12%. Rango and SeaORM decode each row by column name through a generic row type;
  sqlx and Diesel decode by position into a type known at compile time.
- **Diesel-async** is 25–100% slower on writes (`bulk_insert/1000`, `transaction_10_inserts`, `update_one`).
- **Butane** has no multi-row insert: `bulk_insert` is a loop of `save()` in one transaction, one
  round-trip per row, hence ~200× slower at 1000 rows. It has no count API (no `count` row).
  Its suite stores the UUID as `BYTEA` and the timestamp as `TIMESTAMP` (Butane maps no
  `TIMESTAMPTZ`), and uses a single connection: pooling needs its optional `deadpool` feature.
- Clean build of each crate (32 cores): sqlx / Rango / Diesel 11 s, SeaORM 14 s; incremental < 1 s for all.

### Rango optimizations

- Done: decoded rows are collected into a pre-sized `Vec` (−4.5% on `select_all_1000`).
- Measured, not worth it yet: decoding by column index instead of by name saves ~3% on
  1000 rows, at the cost of changing the public `RangoRow` trait.
- Done: `bulk_create` on Postgres is one `INSERT … SELECT FROM UNNEST(…)` statement, with no 65 535-parameter chunking.

## Machine

AMD Ryzen 9 9950X3D 16-Core Processor, 32 threads, WSL2, Postgres 16 in Docker on the same host.

## Methodology

- Criterion defaults: 100 samples, 3 s warmup; one run per stack, run one after another.
- Each suite has its own table, truncated at setup.
