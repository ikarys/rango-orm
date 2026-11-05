# Credentials

## Never commit credentials

The `url` field in `rango.toml` is optional and intended for local development only.
**Do not store production credentials in `rango.toml`** — it will likely end up in version control.

```toml
# rango.toml — safe to commit
[database]
backend = "postgres"
# url is read from DATABASE_URL env var
```

## Use environment variables

```bash
# .env (add to .gitignore)
DATABASE_URL=postgres://user:password@prod-host:5432/mydb

# Or export directly
export DATABASE_URL=postgres://user:password@prod-host:5432/mydb
```

Rango reads `DATABASE_URL` automatically for `rango migrate` and other CLI commands.

## In production

Use your platform's secret management:

```bash
# Docker / Compose
environment:
  - DATABASE_URL=postgres://...

# Kubernetes
env:
  - name: DATABASE_URL
    valueFrom:
      secretKeyRef:
        name: db-secret
        key: url

# systemd
Environment=DATABASE_URL=postgres://...
```

## For tests

Use a separate test database:

```bash
# .env.test
DATABASE_URL=postgres://postgres:postgres@localhost:5432/myapp_test
# or
DATABASE_URL=sqlite::memory:
```

## Checklist

- [ ] `rango.toml` does not contain production credentials
- [ ] `.env` files are in `.gitignore`
- [ ] Test database is separate from production
- [ ] Production URL is injected via environment variable
