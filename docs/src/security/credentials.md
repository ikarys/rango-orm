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

## TLS

Connections to a remote host are encrypted by default: Rango adds `sslmode=require` (PostgreSQL) or `ssl-mode=REQUIRED` (MySQL / MariaDB) when the URL sets no mode. Loopback hosts are left as is. An explicit mode in the URL always wins.

`require` encrypts but does not check the server certificate. In production, set the strict mode in the URL:

```bash
DATABASE_URL=postgres://user:password@db.example.com:5432/mydb?sslmode=verify-full
DATABASE_URL=mysql://user:password@db.example.com:3306/mydb?ssl-mode=VERIFY_IDENTITY
```

## Least privilege

Give the application a role that can only read and write rows, and run migrations with a
separate role that owns the schema. `rango migrate` reads `MIGRATE_DATABASE_URL` first.

Roles are cluster-global, so `CREATE ROLE` fails if the role already exists (create it once, or guard it with a `DO` block that checks `pg_roles`). Schema grants are per database: connect to the target database before running the grants.

```sql
-- Postgres: run as a superuser, connected to the target database
CREATE ROLE app_migrate LOGIN PASSWORD '...';
CREATE ROLE app LOGIN PASSWORD '...';
GRANT CONNECT ON DATABASE mydb TO app_migrate, app;
GRANT CREATE, USAGE ON SCHEMA public TO app_migrate;
GRANT USAGE ON SCHEMA public TO app;
ALTER DEFAULT PRIVILEGES FOR ROLE app_migrate IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO app;
ALTER DEFAULT PRIVILEGES FOR ROLE app_migrate IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO app;
```

```bash
MIGRATE_DATABASE_URL=postgres://app_migrate:...@host/db rango migrate
DATABASE_URL=postgres://app:...@host/db ./my-app
```

`rango dump` and `rango backup` must read every table, including `_rango_migrations`. The DML-only app role is not enough: run them with the migration role (or another role that can read everything).

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

Every claim on this page and the test behind it: [Security requirements](../trust/security-requirements.md).
