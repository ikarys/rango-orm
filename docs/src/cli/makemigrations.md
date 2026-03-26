# rango makemigrations

See [Migrations → makemigrations](../migrations/makemigrations.md) for the full reference.

## Quick reference

```bash
rango makemigrations                  # generate migrations
rango makemigrations --dry-run        # preview SQL without writing
rango makemigrations --check          # CI guard: exit 1 if pending
rango makemigrations --prefix myapp   # custom table prefix
rango makemigrations src/models       # custom source directory
```
