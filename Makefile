DATABASE_URL := postgres://postgres:postgres@localhost:5432/rango_test

.PHONY: help db db-stop test test-all coverage

help:
	@echo "Rango ORM — development commands"
	@echo ""
	@echo "  make db          Start local Postgres (Docker)"
	@echo "  make db-stop     Stop local Postgres"
	@echo "  make test        Run unit tests (no DB required)"
	@echo "  make test-all    Run all tests including integration (requires DB)"
	@echo "  make coverage    Generate coverage report (requires DB)"

db:
	docker compose up -d
	@echo "Waiting for Postgres..."
	@until docker compose exec postgres pg_isready -U postgres > /dev/null 2>&1; do sleep 1; done
	@echo "Postgres ready at $(DATABASE_URL)"

db-stop:
	docker compose down

test:
	cargo test --workspace

test-all: db
	DATABASE_URL=$(DATABASE_URL) cargo test --workspace --features rango-tests/integration

coverage: db
	DATABASE_URL=$(DATABASE_URL) cargo llvm-cov --workspace --features rango-tests/integration --open
