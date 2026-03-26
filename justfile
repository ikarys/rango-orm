# Uses the Geekizz Postgres on 5432, or the rango docker-compose on 5433
database_url := "postgres://geekizz:geekizz@localhost:5432/rango_test"

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
test-all: db
    DATABASE_URL={{database_url}} cargo test --workspace --features rango-tests/integration

# Generate coverage report with HTML output (starts DB if needed)
coverage: db
    DATABASE_URL={{database_url}} cargo llvm-cov --workspace --features rango-tests/integration --open
