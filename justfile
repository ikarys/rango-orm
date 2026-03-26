database_url := "postgres://postgres:postgres@localhost:5433/rango_test"

# Show available commands
help:
    @just --list

# Start local Postgres (Docker) on port 5433
db:
    docker compose up -d
    @echo "Waiting for Postgres..."
    @until docker compose exec postgres pg_isready -U postgres > /dev/null 2>&1; do sleep 1; done
    @echo "Postgres ready at {{database_url}}"

# Stop local Postgres
db-stop:
    docker compose down

# Run fmt + clippy checks (same as pre-push hook)
check:
    cargo fmt --all -- --check
    cargo clippy --workspace -- -D warnings

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
    DATABASE_URL={{database_url}} cargo test --workspace --features rango-tests/integration -- --test-threads=1; \
    docker compose down -v

# Run all tests: unit + sqlite + postgres
test-all: test test-sqlite test-postgres

# Generate coverage report with HTML output
coverage:
    docker compose up -d
    @until docker compose exec postgres pg_isready -U postgres > /dev/null 2>&1; do sleep 1; done
    DATABASE_URL={{database_url}} cargo llvm-cov --workspace \
        --features rango-tests/integration,rango-tests/integration-sqlite \
        --open; \
    docker compose down -v
