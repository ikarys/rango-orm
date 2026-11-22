# Rango ORM — Benchmarks

Same workloads on five stacks, one Postgres: **sqlx raw** (no ORM, baseline), **Rango**,
**SeaORM 2.0**, **Diesel 2.3 + diesel-async 0.9**, **Butane 0.8**. Every read decodes into owned values,
every pool has 10 connections, updates write the full row. Butane differs, see below.

## Run

```bash
just bench-compare   # starts Postgres (Docker), runs the 5 suites, prints the table
```

## Results (PostgreSQL 16 in Docker, 2026-11-22)

Mean time per operation, lower is better. "Rango vs best ORM" compares with the fastest of SeaORM / Diesel / Butane.

| Benchmark | sqlx_raw | rango | seaorm | diesel_async | butane | Rango vs best ORM |
|---|---|---|---|---|---|---|
| `bulk_insert/10` | 2.00 ms | 2.00 ms | 1.99 ms | 2.61 ms | 11.83 ms | +1% |
| `bulk_insert/100` | 2.77 ms | 2.32 ms | 2.43 ms | 3.13 ms | 99.18 ms | -4% |
| `bulk_insert/1000` | 4.62 ms | 4.67 ms | 5.61 ms | 9.36 ms | 978.07 ms | -17% |
| `count` | 880.03 µs | 902.26 µs | 915.23 µs | 900.36 µs | — | +0% |
| `insert_one` | 1.91 ms | 1.97 ms | 1.96 ms | 2.52 ms | 2.09 ms | +0% |
| `select_all_1000` | 1.30 ms | 1.28 ms | 1.42 ms | 1.27 ms | 1.49 ms | +0% |
| `select_filter_100` | 957.73 µs | 974.52 µs | 985.14 µs | 1.01 ms | 1.11 ms | -1% |
| `select_pk` | 876.36 µs | 861.54 µs | 831.04 µs | 861.74 µs | 1.02 ms | +4% |
| `transaction_10_inserts` | 6.63 ms | 6.72 ms | 6.71 ms | 12.27 ms | 12.13 ms | +0% |
| `update_one` | 1.97 ms | 1.94 ms | 1.95 ms | 2.56 ms | 2.04 ms | -0% |

### Reading the numbers

- Every query pays ~0.8 ms of round-trip (Docker on WSL2), so single-row operations are within
  a few percent of each other for sqlx, Rango and SeaORM: that gap is mostly noise (run-to-run variation is ~3–5%).
- **Rango ≈ SeaORM** on every workload.
- `select_all_1000`: decoding 1000 rows into owned structs costs ~0.1–0.2 ms for every stack
  (`fetch_raw_rows_1000` vs `fetch_orm_1000`). The sqlx baseline decodes by column index into a
  tuple — the hand-written best case. Since 2026-11-22 Rango is on par with it (1.28 ms vs
  1.30 ms, within noise), Diesel too; before, Rango decoded each row by column name through a
  `dyn` row type and sat ~12% above. SeaORM still decodes by name.
- **Diesel-async** is 25–100% slower on writes (`bulk_insert/1000`, `transaction_10_inserts`, `update_one`).
- **Butane** has no multi-row insert: `bulk_insert` is a loop of `save()` in one transaction, one
  round-trip per row, hence ~200× slower at 1000 rows. It has no count API (no `count` row).
  Its suite stores the UUID as `BYTEA` and the timestamp as `TIMESTAMP` (Butane maps no
  `TIMESTAMPTZ`), and uses a single connection: pooling needs its optional `deadpool` feature.
- Clean build of each crate (32 cores): sqlx / Rango / Diesel 11 s, SeaORM 14 s; incremental < 1 s for all.

### Rango optimizations

- Done: decoded rows are collected into a pre-sized `Vec` (−4.5% on `select_all_1000`).
- Done (2026-11-22): `select_all_1000` from 1.46 ms to 1.28 ms (sqlx 1.30 ms). The derive
  now emits `FromRow::from_row_in<R: RangoRow>`, so each column read is a static call the
  backend inlines (`#[inline]` on the getters: the benchmarks build without LTO, like most
  user crates); columns are read by position with a name check (`get_*_at(pos, col)`), falling
  back to the name when a custom `SELECT` reorders them; `Option` fields are null-checked at the
  same position; rows are decoded as they stream off the connection instead of being collected
  first. All additive: `from_row`, by-name getters and `PrimaryKey::from_row` keep working.
- Measured, not done (2026-11-22): `try_get_unchecked` (skip sqlx's type-compatibility check per
  column). Decode-only, 1000 rows: 214 µs → 178 µs with Rango's row, 198 µs → 133 µs for
  hand-written sqlx. That is ~3% end to end, for a type mismatch turning from a clear error
  into a decode error (or a silent wrong value for compatible wire formats). Not worth it.
- Done: `bulk_create` on Postgres is one `INSERT … SELECT FROM UNNEST(…)` statement, with no 65 535-parameter chunking.

## Machine

AMD Ryzen 9 9950X3D 16-Core Processor, 32 threads, WSL2, Postgres 16 in Docker on the same host.

## Methodology

- Criterion defaults: 100 samples, 3 s warmup; one run per stack, run one after another.
- Each suite has its own table, truncated at setup.
