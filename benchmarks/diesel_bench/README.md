# diesel_bench with Rango

[diesel_bench](https://github.com/diesel-rs/diesel/tree/main/diesel_bench) is the benchmark suite
the Diesel project uses to compare Rust database crates. `run.sh` clones Diesel at a pinned commit,
applies `diesel_bench.patch` (a `rango-bench` feature, plus a bundled libpq so no system package is
needed) and adds `rango_benches.rs`, the Rango adapter.

```bash
just bench-diesel          # Postgres in Docker, every crate, fast_run sizes
just bench-diesel rango    # Rango only (criterion filter)
DIESEL_BENCH_FEATURES= just bench-diesel   # full sizes (1 to 10 000 rows), much slower
```

## Caveats

- diesel_bench runs on a single-threaded Tokio runtime with one connection. Rango and SeaORM go
  through a sqlx pool, and acquiring from a one-connection pool on a single-threaded runtime costs
  about 300 µs per query there. With a multi-threaded runtime and a few connections the gap is gone.
  Compare small result sets with that in mind.
- `bench_medium_complex_query` is a `LEFT JOIN` in the other adapters. Rango runs two queries
  (users, then posts with `IN (...)`) until it has a join API.
