# Override when 5433 is taken: `RANGO_PG_PORT=5434 just test-postgres`
pg_port := env_var_or_default("RANGO_PG_PORT", "5433")
database_url := "postgres://postgres:postgres@localhost:" + pg_port + "/rango_test"
export RANGO_PG_PORT := pg_port

# Show available commands
help:
    @just --list

# Dev environment: Rango is a library, so this is just the local Postgres
dev: db

# Start local Postgres (Docker) on RANGO_PG_PORT (default 5433)
db:
    docker compose up -d
    @echo "Waiting for Postgres..."
    @until docker compose exec postgres pg_isready -U postgres > /dev/null 2>&1; do sleep 1; done
    @echo "Postgres ready at {{database_url}}"

# Stop local databases
db-stop:
    docker compose --profile mysql down

# Start local MySQL 8.4 (port 3307) and MariaDB 11.4 (port 3308), in-memory storage
db-mysql:
    docker compose --profile mysql up -d --wait mysql mariadb

# MySQL + MariaDB integration tests (requires Docker)
test-mysql: db-mysql
    MYSQL_URL=mysql://root:rango@127.0.0.1:3307/rango_test cargo test --test integration_mysql --features rango-tests/integration-mysql -- --test-threads=1
    MYSQL_URL=mysql://root:rango@127.0.0.1:3307/rango_test cargo test -p rango-orm-cli --test migrations_e2e_mysql --test real_project_mysql --bin rango --features integration-mysql -- --test-threads=1
    MYSQL_URL=mysql://root:rango@127.0.0.1:3308/rango_test cargo test --test integration_mysql --features rango-tests/integration-mysql -- --test-threads=1
    MYSQL_URL=mysql://root:rango@127.0.0.1:3308/rango_test cargo test -p rango-orm-cli --test migrations_e2e_mysql --test real_project_mysql --bin rango --features integration-mysql -- --test-threads=1

# Run fmt + clippy checks (same as pre-push hook)
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Every feature combination of the facade must build (prelude has no ambiguous names)
check-features:
    cargo build -p rango-orm --no-default-features --features sqlite
    cargo build -p rango-orm --no-default-features --features mysql
    cargo build -p rango-orm --no-default-features --features postgres
    cargo test -p rango-orm --all-features --test prelude_backends

# Run SQLite benchmarks (no DB required)
bench-sqlite:
    cargo bench -p rango-bench --bench sqlite

# Run Postgres benchmarks (requires DB)
bench-postgres: db
    DATABASE_URL={{database_url}} cargo bench -p rango-bench --bench postgres; \
    docker compose down -v

# Compare Rango vs sqlx raw, SeaORM and Diesel on Postgres (requires Docker), prints a Markdown table
bench-compare: db
    cd benchmarks && for p in sqlx-raw rango seaorm diesel; do \
        DATABASE_URL={{database_url}} cargo bench -p bench-$p; done
    python3 benchmarks/compare.py

# Run diesel's diesel_bench suite with the Rango adapter (requires Docker); args go to criterion
bench-diesel *args: db
    docker compose exec postgres psql -U postgres -tc "SELECT 1 FROM pg_database WHERE datname = 'diesel_bench'" | grep -q 1 || \
        docker compose exec postgres psql -U postgres -c "CREATE DATABASE diesel_bench"
    DATABASE_URL=postgres://postgres:postgres@localhost:{{pg_port}}/diesel_bench benchmarks/diesel_bench/run.sh {{args}}

# Run unit tests only (no DB required)
test:
    cargo test --workspace

# Run SQLite integration tests (in-memory, no Docker needed)
test-sqlite:
    cargo test --test integration_sqlite --features rango-tests/integration-sqlite -- --test-threads=1

# Run Postgres integration tests (requires Docker)
test-postgres:
    docker compose up -d
    @until docker compose exec postgres pg_isready -U postgres > /dev/null 2>&1; do sleep 1; done
    DATABASE_URL={{database_url}} cargo test --workspace --features rango-tests/integration,rango-orm-cli/integration -- --test-threads=1; \
    docker compose down -v

# Run all tests: unit + sqlite + postgres
test-all: test test-sqlite test-postgres test-mysql

# On-demand end-to-end scenario suite (real blog/CMS app, SQLite in-memory, no Docker).
# Not part of `just test` / CI: run it explicitly when you want to exercise the product.
scenarios:
    cargo build -p rango-orm-cli
    cargo test --test scenarios --features rango-tests/integration-scenarios -- --test-threads=1

# Generate coverage report with HTML output
coverage:
    docker compose up -d
    @until docker compose exec postgres pg_isready -U postgres > /dev/null 2>&1; do sleep 1; done
    DATABASE_URL={{database_url}} cargo llvm-cov --workspace \
        --features rango-tests/integration,rango-tests/integration-sqlite \
        --open; \
    docker compose down -v

# Scan dependencies for known vulnerabilities (RustSec)
audit:
    cargo audit

# Check licenses, banned crates, sources and advisories (deny.toml)
deny:
    cargo deny check
