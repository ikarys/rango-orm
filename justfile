database_url := "postgres://postgres:postgres@localhost:5433/rango_test"

# Show available commands
help:
    @just --list

# Start local Postgres (Docker)
db:
    docker compose up -d
    @echo "Waiting for Postgres..."
    @until docker compose exec postgres pg_isready -U postgres > /dev/null 2>&1; do sleep 1; done
    @echo "Postgres ready at {{database_url}}"

# Stop local Postgres
db-stop:
    docker compose down

# Run unit tests (no DB required)
test:
    cargo test --workspace

# Run all tests including integration (starts DB if needed)
# Run unit tests + integration tests — spins up a dedicated Postgres, runs all tests, tears it down
test-all:
    docker compose up -d
    @echo "Waiting for Postgres..."
    @until docker compose exec postgres pg_isready -U postgres > /dev/null 2>&1; do sleep 1; done
    DATABASE_URL={{database_url}} cargo test --workspace --features rango-tests/integration -- --test-threads=1; \
    docker compose down -v

# Generate coverage report with HTML output (starts DB if needed)
coverage: db
    DATABASE_URL={{database_url}} cargo llvm-cov --workspace --features rango-tests/integration --open
