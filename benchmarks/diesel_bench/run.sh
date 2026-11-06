#!/usr/bin/env bash
# Run diesel's own benchmark suite (diesel_bench) with a Rango adapter added.
# Usage: DATABASE_URL=postgres://... ./run.sh [criterion args, e.g. "rango" to run only Rango]
set -euo pipefail

DIESEL_REV=c11c7e431b52c9f6f25207d6b5b005babc49f0d6
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
WORK=${DIESEL_BENCH_DIR:-$ROOT/target/diesel-bench}

if [ ! -d "$WORK/.git" ]; then
    git clone --filter=blob:none https://github.com/diesel-rs/diesel "$WORK"
fi
cd "$WORK"
git checkout -q --force "$DIESEL_REV"
sed "s#@RANGO@#$ROOT/rango#" "$HERE/diesel_bench.patch" | git apply
cp "$HERE/rango_benches.rs" diesel_bench/benches/

cd diesel_bench
cargo bench --no-default-features --features "${DIESEL_BENCH_FEATURES:-fast_run} postgres \
    sqlx-bench sqlx/postgres sea-orm sea-orm/sqlx-postgres criterion/async_tokio futures \
    diesel-async diesel-async/postgres rango-bench" -- "$@"
