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
- SeaORM → generate from existing DB
- sqlx → manual migrations
- **Rango → change your struct, run `rango makemigrations`. Done.**

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
| `rango-core` | Traits (`Model`, `Backend`), types (`TableSchema`, `Column`, `Migration`), query builder |
| `rango-derive` | `#[derive(Model)]` proc-macro — generates `TableSchema` from struct fields |
| `rango-postgres` | PostgreSQL backend (sqlx) |
| `rango-cli` | `makemigrations` and `migrate` commands |

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
| **Contraintes** | `#[model(unique_together(field1, field2))]`, `CHECK` constraints |
| **Sous-requêtes** | Rango détecte quand une sous-requête est plus efficace qu'un JOIN ou N+1 |
| **select_related** | Charge les FK en un seul JOIN au lieu de N requêtes |
| **Transactions** | `rango::transaction(&pool, \|tx\| async { ... }).await?` |
| **Bulk ops** | `rango::bulk_insert(&pool, vec![...]).await?` |
| **Aggregations** | `.count()` `.sum()` `.avg()` `.min()` `.max()` |
| **Raw SQL escape hatch** | `rango::raw(&pool, "SELECT ...", params).await?` |

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

## Style

- No unsafe
- Minimal dependencies — every dep must earn its place
- Errors are typed, never stringly typed
- Public API is stable-first: think before you expose

---

_Rango is also the name of a chameleon who became a sheriff by accident. We like that energy._
