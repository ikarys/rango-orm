# AGENTS.md — Rango ORM

## Vision

Rango is a Rust ORM inspired by Django ORM — simple, automatic, and opinionated.
The goal: you define your models in Rust, Rango handles everything else.

No SQL. No migration files to write by hand. No boilerplate.

## Philosophy

### 1. Models first
You define a struct. Rango infers the database schema from it.
No external config files, no schema.prisma, no XML. Just Rust.

```rust
#[derive(Model)]
struct Article {
    id: Uuid,
    title: String,
    body: Text,
    published: bool,
    created_at: DateTime,
}
```

### 2. Migrations are generated, never written
`rango makemigrations` — diffs current models against last known state, generates numbered `.sql` files.
`rango migrate` — applies pending migrations in order, reads `DATABASE_URL` or `rango.toml`.

You never touch migration files unless you want to. They're generated artifacts.

This is the **#1 differentiator** vs other Rust ORMs:
- Diesel → write SQL migrations by hand
- SeaORM → write migrations in Rust by hand (entity-first schema sync is newer and less mature)
- sqlx → manual migrations
- Butane → auto-generated migrations too, but a small, less active ecosystem
- **Rango → Django-style: change your struct, run `rango makemigrations`. Done. Async, ergonomic, production-grade.**

### 3. Zero ceremony
- No `impl Model for Article { ... }` by hand
- No `#[column(name = "...", type = "...", nullable = false)]` on every field
- Convention over configuration — sensible defaults, override only when needed

### 4. Where Django fell short (and we aim higher)

| Django pain point | Rango goal |
|---|---|
| Migrations can conflict in teams (merge hell) | Deterministic diff based on snapshots — conflicts are explicit |
| `makemigrations` sometimes generates wrong SQL | Schema snapshots are typed, not text-diffed |
| QuerySet API is stringly typed | Query builder is fully typed, compile-time checked |
| `related_name` and FK hell | Relations declared explicitly, no magic reverse managers |
| `null=True` / `blank=True` confusion | One concept: `Option<T>` |
| `on_delete=CASCADE` is fake — Django issues individual DELETE queries per row in Python | Real SQL `ON DELETE CASCADE` — let the DB do it, zero round-trips |
| Complex `AND`/`OR` conditions are awkward (`Q` objects, verbose nesting) | First-class typed `and()` / `or()` / `not()` in the query builder |
| Multi-table JOINs are implicit and hard to control | Explicit `.join()` with full control over join type and condition |

### 5. Backend agnostic
Core traits are DB-agnostic. Backends (`rango-orm-postgres`, future `rango-orm-sqlite`, etc.) are separate crates.
This is by design — Rango should be portable.

## Crate layout

| Crate | Role |
|---|---|
| `rango-orm-core` | DB-free: traits (`Model`, `FromRow`, `ModelValues`), schema types (`TableSchema`, `ColumnDef`, constraints), field types, `quote_ident` |
| `rango-orm-derive` | `#[derive(Model)]` proc-macro — generates `TableSchema` from struct fields |
| `rango-orm-postgres` | PostgreSQL backend (sqlx): `ops/` (crud, bulk, raw), `query_builder/` (filter, exec, relations), M2M, `PgQueryExt` |
| `rango-orm-sqlite` | SQLite backend (sqlx), same layout as `rango-orm-postgres` |
| `rango-orm-mysql` | MySQL 8.0+ / MariaDB 10.6+ backend (sqlx), common subset, no `RETURNING`, UUID as `BINARY(16)` |
| `rango-orm` | Facade crate (lib name `rango`): re-exports core + derive + backends, selected by features (`postgres` default, `sqlite`, `mysql`) |
| `rango-orm-cli` | `rango` binary: `commands/` (init, makemigrations, migrate, export, import) and `migration/` (scanner, snapshot, diff, renames, sql) |
| `rango-tests` | Integration tests against real Postgres / SQLite |
| `rango-bench` | Criterion benchmarks |

## CLI — commandes prévues

| Commande | Description |
|---|---|
| `rango makemigrations` | Génère les fichiers de migration depuis les modèles |
| `rango migrate` | Applique les migrations en attente |
| `rango dump` | Export complet de la DB (schema + data) |
| `rango dump --schema-only` | Export du schéma uniquement |
| `rango dump --data-only` | Export des données uniquement |
| `rango dump --table <name>` | Export d'une table spécifique |
| `rango restore <file>` | Restaure depuis un dump |
| `rango backup` | Snapshot compressé horodaté (dump + compression automatique) |

L'objectif : ne plus jamais avoir à retourner sur la doc de `pg_dump` / `mysqldump`.
Une commande, ça marche pareil quel que soit le backend.

## Model Hooks

Opt-in trait `ModelHooks` (supertrait of `Model`). `#[derive(Model)]` emits an empty impl; with
`#[model(hooks)]` you write `impl ModelHooks for X { ... }` yourself.

```rust
#[derive(Model)]
#[model(hooks)]
struct User {
    id: FieldUuid,
    name: FieldVarchar<1, 100>,
    slug: FieldVarchar<0, 100>,
}

impl ModelHooks for User {
    fn before_save(&mut self) -> BoxFuture<'_> {
        Box::pin(async move {
            self.slug = slugify(&self.name);
            Ok(())
        })
    }
}
```

Available hooks (all have a no-op default impl): `before_save` / `after_save` (INSERT or UPDATE),
`before_create` / `after_create` (INSERT), `before_update` / `after_update` (UPDATE),
`before_delete` / `after_delete` (DELETE).

Design rules:
- Fired by `insert`, `update`, `delete`, `get_or_create` (create hooks only when it inserts; `before_*` run on `defaults` first) and `update_or_create`. Order: `before_save`, `before_create|update`, SQL, `after_create|update`, `after_save`.
- Not fired: bulk ops, query-builder updates/deletes, `raw*` (same as Django).
- Hooks return `HookResult`; an `Err` becomes `Error::Hook`. `before_*` Err: no SQL runs. `after_*` Err: returned to the caller, and rolled back only inside `atomic()`/a transaction (a plain pool has already committed).
- Hooks do not open a transaction themselves.

## Roadmap

### Done (all three backends)

| Feature | Notes |
|---|---|
| **Query builder** ✅ | `eq/ne/gt/gte/lt/lte/like/in_values/is_null`, `or()` / `not()` / `xor()` / `group()`, `order_by`, `limit`/`offset`, default cap of 1000 rows (`unlimited()`), `.explain()`. Typed columns (`User::EMAIL`) checked at compile time; string names checked against `M::schema()` at run time (`Error::Invalid`) |
| **Relations FK** ✅ | `ForeignKey<User>` → real `REFERENCES … ON DELETE` in SQL, FK columns indexed |
| **ManyToMany** ✅ | Auto-generated pivot table; `add` / `remove` / `set` / `all` / `reverse` / `clear` / `exists` |
| **Constraints** ✅ | `#[model(constraints = [CheckConstraint::new("age >= 18").name("chk_age"), UniqueConstraint::on(&["room", "date"]).name("uniq_booking")])]`: `CHECK` and `UNIQUE` inline in `CREATE TABLE`; `UNIQUE … WHERE cond` → partial unique index (Postgres, SQLite; skipped with a comment on MySQL). `managed = false` → ignored by migrations. `ordering = [asc("col"), desc("other")]` → default order |
| **Subqueries** ✅ | `where_exists(Article::AUTHOR_ID, \|q\| ..)` (correlated `EXISTS`, `.not()` → `NOT EXISTS`) and `in_subquery(col, sub_col, \|q\| ..)`; decision rules for subquery vs `JOIN` vs `prefetch` documented in `docs/src/querying/filter.md`. Left (P2): Rango auto-picks the cheapest shape from a cost model |
| **select_related** ✅ | One `LEFT JOIN` (filtered/paginated M in a subquery, R's columns prefixed `__r{i}_`); `all()` returns `Vec<WithRelated<M>>` |
| **prefetch_related** ✅ | Reverse FK (`Comment::POST_ID`) and M2M (`Article::TAGS`, one `INNER JOIN` on the pivot); `= ANY($1)` on Postgres, `IN (...)` in batches of 900 elsewhere |
| **values** ✅ | `.values(&["id", "email"])` → `Vec<HashMap<String, SqlValue>>` |
| **Aggregations** ✅ | `.count()` `.exists()` `.sum()` `.avg()` `.min()` `.max()` |
| **Transactions** ✅ | `rango::atomic/transaction(&pool, async \|tx\| { ... }).await?` (async closure, `Send` future); crud, `filter`, M2M and bulk ops accept a pool or a transaction (`&mut **tx`) on all three backends; `get_or_create` is atomic (`INSERT … ON CONFLICT DO NOTHING RETURNING *` on Postgres); `update_or_create` on every backend; `atomic_retry` on Postgres |
| **Bulk ops** ✅ | `rango::bulk_create/update/upsert(&pool, &models, ...)`: multi-row `INSERT`, `UPDATE … FROM (VALUES …)` (per-row `UPDATE` in one transaction on SQLite and MySQL), upsert; all three backends; atomic (savepoint inside a transaction); auto-chunked under the parameter limit; pool or transaction |
| **Raw SQL escape hatch** ✅ | `rango::raw` / `raw_scalar` / `raw_execute(&pool, "SELECT ...", params)`, values always bound |
| **Auto-increment PK** ✅ | `FieldAutoInt` / `FieldAutoBigInt`, id assigned by the database |
| **Partial selects (only / defer)** ✅ | `#[derive(Partial)] #[partial(model = User)] struct UserLite { .. }` + `.select_as::<UserLite>()`; columns and types checked at compile time. Not a Model with defaults (`update()` would overwrite), not a `HashMap` (that is `.values()`) |
| **Implicit PK** ✅ | `#[rango::model]` attribute macro injects `pub id: FieldUuid` (`pk = "bigint"` / `"int"` → auto-increment) and derives `Model`. Per model, not in `rango.toml` (cargo does not track it) |
| **CLI dump / restore / backup** ✅ | Rango JSON (schema snapshot + rows), backend-independent; restore in one transaction, input untrusted; backup = gzip + UTC timestamp, `0600` |
| **Keyset pagination** ✅ | `.after(col, value)` / `.before(col, value)` |
| **Stored procedures** ✅ | No `rango::call`: `raw_scalar` / `raw` cover `CALL` and set-returning functions (tested on PG, MySQL, MariaDB). Create procedures in a migration file (MySQL cannot prepare `CREATE PROCEDURE`) |

## Principes non-négociables

- **100% async** — toutes les ops DB sont async, pas de blocking
- **Performance** — pool de connexions, RETURNING * pour éviter les re-fetch, JOIN au lieu de N+1
- **Concurrence** — tous les types doivent être `Send + Sync`, compatible tokio multi-thread
- **Modularité** — chaque backend est une crate séparée, le core n'a aucune dep DB
- **Simplicité d'utilisation** — la complexité est dans Rango, pas dans le code utilisateur

### Sous-requêtes — philosophie

Rango doit être **intelligent** sur le SQL généré :
- Détecter les N+1 et les remplacer automatiquement par des sous-requêtes ou JOINs
- Pour les `filter()` sur des relations, utiliser `EXISTS (subquery)` plutôt que des JOINs quand c'est plus efficace
- Exposer `.explain()` pour que le dev puisse voir le SQL généré

```rust
// Rango génère automatiquement une sous-requête efficace
let active_users = User::filter(&pool)
    .where_exists(Article::AUTHOR_ID, |q| q.eq(Article::PUBLISHED, true))  // sous-requête
    .all()
    .await?;
// → SELECT * FROM "user" WHERE EXISTS (SELECT 1 FROM "article" WHERE published = true AND user_id = "user".id)
```

## Non-goals (for now)

- Async streaming / cursors
- Multi-DB joins across backends
- Admin UI (that's Django's job, not ours)
- ORM for NoSQL

## Backends — feuille de route

| Backend | Crate | Status |
|---|---|---|
| PostgreSQL | `rango-orm-postgres` | ✅ backend + CLI (default backend) |
| MySQL / MariaDB | `rango-orm-mysql` | ✅ backend + CLI (MySQL 8.0+, MariaDB 10.6+) |
| SQLite | `rango-orm-sqlite` | ✅ |
| MSSQL | `rango-mssql` | 🔮 futur — SQLx a droppé le support MSSQL en 0.7, backend custom via `tiberius` |

Note : PostgreSQL, MySQL, SQLite passent par SQLx. MSSQL nécessitera un backend séparé basé sur `tiberius`.

## Security Model

### SQL Injection

**Safe by default.** Every QueryBuilder operation uses prepared statements with bound parameters.
Values are never interpolated into SQL strings — the database driver handles escaping.

```rust
// Safe — "email" is bound as a parameter, not interpolated
User::filter(&pool).eq("email", user_input).all().await?;
// → SELECT * FROM "user" WHERE "email" = $1  (Postgres)
// → SELECT * FROM "user" WHERE "email" = ?   (SQLite)
```

**`raw()` is your responsibility.** When you write raw SQL, Rango cannot protect you:

```rust
// ❌ DANGEROUS — never do this
let sql = format!("SELECT * FROM users WHERE name = '{}'", user_input);
rango::raw_execute(&pool, &sql, vec![]).await?;

// ✅ SAFE — always use parameters
rango::raw_execute(&pool, "SELECT * FROM users WHERE name = $1",
    vec![SqlValue::Text(user_input)]).await?;
```

### Credentials

**Never store production credentials in `rango.toml`.**
The `url` field is optional and intended for local development only.
In production, always use the `DATABASE_URL` environment variable:

```bash
# .env (never commit this)
DATABASE_URL=postgres://user:password@prod-host:5432/mydb
```

`rango.toml` should only contain the `backend` field in production:

```toml
[database]
backend = "postgres"
# url is read from DATABASE_URL env var
```

### Backend-specific features

Some features are engine-specific (Postgres JSON operators, full-text search, etc.).
These are exposed via backend-specific traits (`PgQueryExt`, `SqliteQueryExt`) — not the generic QueryBuilder.
If you use them, your code is explicitly tied to that backend. No silent portability issues.

## Hardening plan — security, observability, performance

Goal: production-grade by default. Status as of 2026-11-01. P0 = blocks real use, P1 = needed for production, P2 = next.

### Security

| Prio | Item | Status / notes |
|---|---|---|
| P0 | Identifier escaping — every table/column name goes through `quote_ident` (doubles `"`) | ✅ query builder, ops, CLI SQL generation, `import`/`export` (names from files and the catalog are untrusted) |
| P0 | `FieldPassword` stores plaintext, derives `Debug` + `Serialize` | ✅ argon2id hash on write, redacted `Debug`, no serde, `verify(&self, candidate)` |
| P0 | `DatabaseConfig` derives `Debug` → password in logs | ✅ manual `Debug` with `redact_url`; CLI config has no `Debug` |
| P0 | TLS for Postgres and MySQL (`sslmode=require/verify-full`, `ssl-mode=REQUIRED/VERIFY_IDENTITY`) | ✅ `tls-rustls`; remote hosts default to `require` / `REQUIRED` when the URL sets no mode (no silent cleartext fallback); use `verify-full` / `VERIFY_IDENTITY` in prod |
| P1 | Validate filter/order columns against `M::schema()` | ✅ string column names (filters, `order_by`, subqueries, `values`, aggregates, backend extensions) → `Error::Invalid` before any SQL runs; typed columns are already checked at compile time |
| P1 | Per-connection `statement_timeout` / `lock_timeout` in `DatabaseConfig` | ✅ `statement_timeout_ms` / `lock_timeout_ms`, sent in the startup packet (no extra round-trip; PgBouncer needs `ignore_startup_parameters`) |
| P1 | Never log bound values by default | ✅ sqlx only logs SQL text, never bound values; no opt-in added |
| P1 | Typed errors (`rango::Error`: `NotFound`, `UniqueViolation`, `ForeignKeyViolation`, `Timeout`, `Connection`, …) instead of `anyhow` in library crates | ✅ `rango_orm_core::Error`; each backend maps sqlx errors in `error.rs` (`db_err`); no SQL text in errors; `anyhow` only in `rango-orm-cli` |
| P1 | `#![forbid(unsafe_code)]` in every crate; `cargo audit` + `cargo deny` in CI | ✅ `forbid(unsafe_code)` in every crate root ✅; `cargo audit` in CI + `just audit` (exceptions in `.cargo/audit.toml`) ✅; `cargo deny` (`deny.toml`: licenses, sources, advisories) in CI + `just deny` ✅ |
| P2 | Least-privilege guide: a migration role (DDL) separate from the app role (DML only) | ✅ `rango migrate` reads `MIGRATE_DATABASE_URL` before `DATABASE_URL`; guide in `docs/src/security/credentials.md` |
| P2 | `dump` / `backup` files written with `0600` permissions; `restore` refuses non-Rango dumps | ✅ |

Developer-controlled strings (`CheckConstraint` SQL, partial-unique conditions, `#[field(default = "...")]`, `raw*`) are trusted SQL by design. Document them as such; never build them from user input.

Known-CVE checklist (reviewed 2026-11-01) — keep new APIs out of these classes:

| Class | Reference CVEs | Rango rule |
|---|---|---|
| Unescaped / badly escaped identifiers (`order_by`, aliases, `sql.identifier`) | Django CVE-2021-35042, CVE-2026-1312, CVE-2026-1287; Drizzle CVE-2026-39356 | Only `quote_ident` builds identifiers; never `format!("\"{}\"")` |
| JSON key / path interpolated into SQL | Django CVE-2024-42005; Sequelize CVE-2026-30951 | JSON keys, paths, FTS configs and array values are bound parameters |
| Query structure from user dicts (`**kwargs`, `_connector`, nested objects) | Django CVE-2025-64459, CVE-2025-59681; TypeORM CVE-2025-60542 | Operators and connectors are Rust enums, never strings from input |
| Protocol length overflow (> 4 GiB values) | RUSTSEC-2024-0363 (sqlx), RUSTSEC-2024-0365 (diesel) | sqlx ≥ 0.8.1 required (`cargo audit` in CI) |
| Object names from a dump turned into SQL on restore | PostgreSQL CVE-2025-8714, CVE-2025-8715 | `import`/`restore` treat file contents as untrusted: names quoted, values bound; covered by an e2e test |
| TLS downgrade (`sslmode=prefer`, MySQL `PREFERRED`) | — | Remote hosts default to `require` / `REQUIRED` |
| Bundled SQLite engine bugs | SQLite CVE-2025-6965, CVE-2025-29087 (fixed in 3.50.2 / 3.49.1) | Need attacker-controlled SQL, which Rango never runs; bump `libsqlite3-sys` with sqlx (0.8 bundles 3.46) |

### Observability

| Prio | Item | Notes |
|---|---|---|
| P1 | `tracing` spans per query: `db.system`, `db.operation`, `db.sql.table`, rows, duration (OpenTelemetry semantic conventions) | ✅ `db.query` span on every operation (CRUD, query builder, bulk, M2M, raw); sqlx adds one event per statement on `sqlx::query` |
| P1 | Slow query log: `slow_query_threshold` in config (default 500 ms) → `WARN` with SQL (no values) and duration | ✅ `slow_query_threshold_ms` (Postgres), fixed 500 ms on SQLite |
| P1 | Pool metrics: `pool.size()`, idle, wait time on acquire | sqlx already exposes `pool.size()` / `pool.num_idle()`; sqlx 0.8 logs a `WARN` on acquires slower than 2 s (`acquire_slow_threshold`) ✅ |
| P2 | N+1 detector (debug builds): warn when the same query shape runs > N times within a span | Matches the "Rango detects N+1" vision |
| P2 | `.explain_analyze()` next to `.explain()` | ✅ PG/MySQL `EXPLAIN ANALYZE`, MariaDB `ANALYZE FORMAT=JSON`, SQLite `EXPLAIN QUERY PLAN` (no timings) |

### Performance

| Prio | Item | Notes |
|---|---|---|
| P0 | Index every FK column, index M2M pivots, real `REFERENCES … ON DELETE` | ✅ derive marks every FK `index`; CLI emits `CREATE INDEX` on create/add column and on pivots |
| P0 | `rango-orm-core` must not depend on sqlx | ✅ core has no DB dependency; each backend crate brings its own sqlx driver |
| P1 | Pool tuning: `test_before_acquire(false)` (sqlx pings on every acquire by default), `max_lifetime`, configurable from `rango.toml` | ✅ `max_lifetime` (1800 s), `test_before_acquire` (false) in `DatabaseConfig`; not in `rango.toml` by design (CLI-only file, apps build `DatabaseConfig`) |
| P2 | Row decoding by column index instead of by name | Measured: ~3% on `select_all_1000` (name lookup is cheap); needs a `RangoRow` API change. Pre-sized result `Vec` ✅ (−4.5%) |
| P1 | Postgres `bulk_create` via `UNNEST` (one statement, no 65 535-param chunking) | ✅ JSON columns fall back to chunked multi-row `VALUES` |
| P1 | Keyset pagination helper (`.after(col, value)`) | ✅ `.after()` / `.before()` |
| P1 | Transaction retry on serialization failure / deadlock (`40001`, `40P01`) in `atomic()`, opt-in, bounded | ✅ `atomic_retry(&pool, max_attempts, f)` (Postgres), exponential backoff without jitter |
| P1 | SQLite table rebuild for ALTER TYPE / NULL / FK changes (create new → copy → drop → rename, in one transaction) | ✅ `RebuildTable` diff on SQLite; `migrate` turns FKs off and runs `PRAGMA foreign_key_check` in each migration transaction |
| P2 | Optimistic locking (`#[field(version)]` → `UPDATE … WHERE version = $n`) | Safe concurrent updates without row locks |
| P2 | Read-replica routing (writes → primary, reads → replica) | |

## Known limitations

- SQLite has no decimal type: `FieldDecimal` is stored in a `NUMERIC` column, so a value with more than 15 significant digits is kept exactly as text, and one with up to 15 digits is a number again once read (scale restored from the column). Postgres and MySQL are exact.
- A Postgres FK cycle is restorable because the FKs that close it are `DEFERRABLE`; databases created by an older `rango migrate` have them non-deferrable, so a NOT NULL cycle there still needs the constraints recreated (`ALTER TABLE ... ALTER CONSTRAINT ... DEFERRABLE`).

## Style

- No unsafe
- Minimal dependencies — every dep must earn its place
- Errors are typed, never stringly typed
- Public API is stable-first: think before you expose

---

_Rango is also the name of a chameleon who became a sheriff by accident. We like that energy._
