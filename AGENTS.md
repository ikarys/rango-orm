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
Core traits are DB-agnostic. Backends (`rango-postgres`, future `rango-sqlite`, etc.) are separate crates.
This is by design — Rango should be portable.

## Crate layout

| Crate | Role |
|---|---|
| `rango-core` | DB-free: traits (`Model`, `FromRow`, `ModelValues`), schema types (`TableSchema`, `ColumnDef`, constraints), field types, `quote_ident` |
| `rango-derive` | `#[derive(Model)]` proc-macro — generates `TableSchema` from struct fields |
| `rango-postgres` | PostgreSQL backend (sqlx): `ops/` (crud, bulk, raw), `query_builder/` (filter, exec, relations), M2M, `PgQueryExt` |
| `rango-sqlite` | SQLite backend (sqlx), same layout as `rango-postgres` |
| `rango` | Facade crate: re-exports core + derive + backends, selected by features (`postgres` default, `sqlite`) |
| `rango-cli` | `rango` binary: `commands/` (init, makemigrations, migrate, export, import) and `migration/` (scanner, snapshot, diff, renames, sql) |
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

Opt-in trait `ModelHooks` — implement only what you need.

```rust
#[derive(Model)]
struct User {
    id: FieldUuid,
    name: FieldVarchar<1, 100>,
    slug: FieldVarchar<1, 100>,
}

impl ModelHooks for User {
    async fn before_save(&mut self) -> Result<()> {
        self.slug = slugify(&self.name);
        Ok(())
    }

    async fn after_save(&self) -> Result<()> {
        send_welcome_email(self).await?;
        Ok(())
    }
}
```

Available hooks (all have a no-op default impl):

| Hook | Triggered |
|---|---|
| `before_save` | Before INSERT or UPDATE |
| `after_save` | After INSERT or UPDATE |
| `before_create` | Before INSERT only |
| `after_create` | After INSERT only |
| `before_update` | Before UPDATE only |
| `after_update` | After UPDATE only |
| `before_delete` | Before DELETE |
| `after_delete` | After DELETE |

Design rules:
- All hooks are `async`
- All hooks return `Result<()>` — returning `Err` cancels the operation
- Hooks run inside the same transaction as the DB operation
- `ModelHooks` is opt-in — no impl needed if unused

## Roadmap

### À implémenter (par ordre de priorité)

| Feature | Description |
|---|---|
| **Query builder** | `.filter().eq().or().order_by().limit().offset()` |
| **Relations FK** | `ForeignKey<User>` → vrai `ON DELETE CASCADE` SQL |
| **ManyToMany** | Table pivot auto-générée, `.add()` `.remove()` `.all()` |
| **Contraintes** ✅ | `CheckConstraint`, `UniqueConstraint` (simple et partial), `managed`, `ordering` — voir ci-dessous |
| **Sous-requêtes** | Rango détecte quand une sous-requête est plus efficace qu'un JOIN ou N+1 |
| **select_related** | Charge les FK en un seul JOIN au lieu de N requêtes |
| **prefetch_related** | Charge les M2M en une requête séparée optimisée |
| **only / defer** | `.only(&["id", "email"])` / `.defer(&["password"])` → retourne type partiel ou `HashMap` |
| **values** | `.values(&["id", "email"])` → `Vec<HashMap<String, SqlValue>>` |
| **Contraintes** ✅ | `#[model(constraints = [CheckConstraint::new("age >= 18").name("chk_age"), UniqueConstraint::on(&["room", "date"]).name("uniq_booking")])]` — `CHECK (...)` inline dans `CREATE TABLE`; `UNIQUE (cols)` inline; `UNIQUE ... WHERE cond` → `CREATE UNIQUE INDEX ... WHERE ...` (partial index PostgreSQL). `managed = false` → table ignorée par les migrations. `ordering = [asc("col"), desc("other")]` → ordre par défaut injecté dans `schema()`. |
| **Transactions** ✅ | `rango::atomic/transaction(&pool, \|tx\| async { ... }).await?` — `get/all/insert/update/delete` accept any `Executor`; `get_or_create` is atomic via `INSERT … ON CONFLICT DO NOTHING RETURNING *` and accepts any `Acquire` (pool or transaction, no `atomic()` needed); `update_or_create` accepts `&mut RangoExecutor`; `QueryBuilder::transaction()` available |
| **Bulk ops** ✅ | `rango::bulk_create/update/upsert(&pool, &models, ...)` — multi-row INSERT, `UPDATE … FROM (VALUES …)`, INSERT ON CONFLICT; auto-chunked at 65 535 params; accept pool or transaction via `Acquire` |
| **Aggregations** | `.count()` `.sum()` `.avg()` `.min()` `.max()` |
| **Raw SQL escape hatch** | `rango::raw(&pool, "SELECT ...", params).await?` |
| **Stored procedures** | `rango::call(&pool, "proc_name", params).await?` — MySQL/PG stored procs. À évaluer : utilité dans un ORM ? |
| **Auto PK** | Si aucun champ `id` déclaré, Rango en génère un automatiquement. Type configurable dans `rango.toml` (`[models] default_pk = "uuid"` ou `"bigint"`). L'utilisateur peut déclarer le sien avec `#[field(primary_key)]`. |

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
    .exists(Article::filter().eq("published", true))  // sous-requête
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
| PostgreSQL | `rango-postgres` | 🚧 en cours |
| MySQL / MariaDB | `rango-mysql` | 📋 prévu |
| SQLite | `rango-sqlite` | 📋 prévu |
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
| P0 | TLS for Postgres (`sslmode=require/verify-full`) | ✅ `tls-rustls`; remote hosts default to `require` when no `sslmode` is given (no silent cleartext fallback); use `verify-full` in prod |
| P1 | Validate filter/order columns against `M::schema()` | Unknown column → typed error instead of a DB error; defense in depth on top of escaping |
| P1 | Per-connection `statement_timeout` / `lock_timeout` in `DatabaseConfig` | ✅ `statement_timeout_ms` / `lock_timeout_ms`, sent in the startup packet (no extra round-trip; PgBouncer needs `ignore_startup_parameters`) |
| P1 | Never log bound values by default | ✅ sqlx only logs SQL text, never bound values; no opt-in added |
| P1 | Typed errors (`rango::Error`: `NotFound`, `UniqueViolation`, `ForeignKeyViolation`, `Timeout`, `Connection`, …) instead of `anyhow` in library crates | ✅ `rango_core::Error`; each backend maps sqlx errors in `error.rs` (`db_err`); no SQL text in errors; `anyhow` only in `rango-cli` |
| P1 | `#![forbid(unsafe_code)]` in every crate; `cargo audit` + `cargo deny` in CI | 🚧 `forbid(unsafe_code)` in every crate root ✅; `cargo audit` in CI + `just audit` (exceptions in `.cargo/audit.toml`) ✅; `cargo deny` pending |
| P2 | Least-privilege guide: a migration role (DDL) separate from the app role (DML only) | `rango migrate` can take its own `MIGRATE_DATABASE_URL` |
| P2 | `dump` / `backup` files written with `0600` permissions; `restore` refuses non-Rango dumps | |

Developer-controlled strings (`CheckConstraint` SQL, partial-unique conditions, `#[field(default = "...")]`, `raw*`) are trusted SQL by design. Document them as such; never build them from user input.

Known-CVE checklist (reviewed 2026-11-01) — keep new APIs out of these classes:

| Class | Reference CVEs | Rango rule |
|---|---|---|
| Unescaped / badly escaped identifiers (`order_by`, aliases, `sql.identifier`) | Django CVE-2021-35042, CVE-2026-1312, CVE-2026-1287; Drizzle CVE-2026-39356 | Only `quote_ident` builds identifiers; never `format!("\"{}\"")` |
| JSON key / path interpolated into SQL | Django CVE-2024-42005; Sequelize CVE-2026-30951 | JSON keys, paths, FTS configs and array values are bound parameters |
| Query structure from user dicts (`**kwargs`, `_connector`, nested objects) | Django CVE-2025-64459, CVE-2025-59681; TypeORM CVE-2025-60542 | Operators and connectors are Rust enums, never strings from input |
| Protocol length overflow (> 4 GiB values) | RUSTSEC-2024-0363 (sqlx), RUSTSEC-2024-0365 (diesel) | sqlx ≥ 0.8.1 required (`cargo audit` in CI) |
| Object names from a dump turned into SQL on restore | PostgreSQL CVE-2025-8714, CVE-2025-8715 | `import`/`restore` treat file contents as untrusted: names quoted, values bound; covered by an e2e test |
| TLS downgrade (`sslmode=prefer`) | — | Remote hosts default to `require` |
| Bundled SQLite engine bugs | SQLite CVE-2025-6965, CVE-2025-29087 (fixed in 3.50.2 / 3.49.1) | Need attacker-controlled SQL, which Rango never runs; bump `libsqlite3-sys` with sqlx (0.8 bundles 3.46) |

### Observability

| Prio | Item | Notes |
|---|---|---|
| P1 | `tracing` spans per query: `db.system`, `db.operation`, `db.sql.table`, rows, duration (OpenTelemetry semantic conventions) | 🚧 sqlx already emits one event per query on target `sqlx::query` (`db.statement`, `rows_affected`, `rows_returned`, `elapsed`); Rango-level spans with `db.sql.table` pending |
| P1 | Slow query log: `slow_query_threshold` in config (default 500 ms) → `WARN` with SQL (no values) and duration | ✅ `slow_query_threshold_ms` (Postgres), fixed 500 ms on SQLite |
| P1 | Pool metrics: `pool.size()`, idle, wait time on acquire | sqlx already exposes `pool.size()` / `pool.num_idle()`; acquire wait time pending |
| P2 | N+1 detector (debug builds): warn when the same query shape runs > N times within a span | Matches the "Rango detects N+1" vision |
| P2 | `.explain_analyze()` next to `.explain()` | |

### Performance

| Prio | Item | Notes |
|---|---|---|
| P0 | Index every FK column, index M2M pivots, real `REFERENCES … ON DELETE` | ✅ derive marks every FK `index`; CLI emits `CREATE INDEX` on create/add column and on pivots |
| P0 | `rango-core` must not depend on sqlx (currently pulls postgres + mysql + sqlite drivers) | Core stays pure; compile time and binary size drop for every app |
| P1 | Pool tuning: `test_before_acquire(false)` (sqlx pings on every acquire by default), `max_lifetime`, configurable from `rango.toml` | ✅ `max_lifetime` (1800 s), `test_before_acquire` (false) in `DatabaseConfig`; not yet in `rango.toml` |
| P2 | Row decoding by column index instead of by name | Measured: ~3% on `select_all_1000` (name lookup is cheap); needs a `RangoRow` API change. Pre-sized result `Vec` ✅ (−4.5%) |
| P1 | Postgres `bulk_create` via `UNNEST` (one statement, no 65 535-param chunking) | +21% at 1000 rows in benchmarks |
| P1 | Keyset pagination helper (`.after(col, value)`) | `OFFSET` is O(n) on large tables |
| P1 | Transaction retry on serialization failure / deadlock (`40001`, `40P01`) in `atomic()`, opt-in, bounded | ✅ `atomic_retry(&pool, max_attempts, f)` (Postgres), exponential backoff without jitter |
| P1 | SQLite table rebuild for ALTER TYPE / NULL / FK changes (create new → copy → drop → rename, in one transaction) | Same migration UX on every backend; today SQLite only gets a comment |
| P2 | Optimistic locking (`#[field(version)]` → `UPDATE … WHERE version = $n`) | Safe concurrent updates without row locks |
| P2 | Read-replica routing (writes → primary, reads → replica) | |

## Style

- No unsafe
- Minimal dependencies — every dep must earn its place
- Errors are typed, never stringly typed
- Public API is stable-first: think before you expose

---

_Rango is also the name of a chameleon who became a sheriff by accident. We like that energy._
