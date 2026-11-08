# Filter & Lookup

The query builder is chainable, lazy, and type-safe.

## Basic filtering

```rust
// All active users
let users = User::filter(&pool)
    .eq("active", true)
    .all()
    .await?;

// Users with score > 100
let users = User::filter(&pool)
    .gt("score", 100)
    .all()
    .await?;
```

## Typed columns

`#[derive(Model)]` generates one constant per field (`User::EMAIL`, `User::SCORE`, …),
typed by model and field type (`Column<User, FieldInt>`). Use them instead of strings:
a typo, a renamed field, a column from another model, or a value of the wrong type
becomes a compile error instead of a database error.

```rust
let users = User::filter(&pool)
    .eq(User::ACTIVE, true)
    .gt(User::SCORE, 100)
    .order_by(User::CREATED_AT.desc())
    .all()
    .await?;

let total = Order::filter(&pool).sum(Order::PRICE).await?;

// Compile errors:
// User::filter(&pool).eq(Article::TITLE, "x");    // column of another model
// User::filter(&pool).eq(User::SCORE, "high");    // `&str` is not a `FieldInt` value
// User::filter(&pool).eq(User::PASSWORD, "pw");   // passwords are salted hashes
```

A typed column accepts its field type or the plain Rust equivalent: `i32` for `FieldInt`,
`i64` for `FieldBigInt` and `FieldRange`, `&str`/`String` for `FieldText`, `FieldEmail`,
`FieldUrl` and `FieldVarchar`, `Uuid` or `FieldUuid` for `ForeignKey<M>`.
Filter values are not validated like field constructors: searching for
`"not-an-email"` is allowed.

String names (`"email"`, `"-created_at"`) are still accepted everywhere; they accept any
value and are checked by the database at runtime.

## Operators

| Method | SQL | Example |
|---|---|---|
| `.eq(col, val)` | `col = val` | `.eq("role", "admin")` |
| `.ne(col, val)` | `col != val` | `.ne("status", "banned")` |
| `.gt(col, val)` | `col > val` | `.gt("age", 18)` |
| `.gte(col, val)` | `col >= val` | `.gte("score", 100)` |
| `.lt(col, val)` | `col < val` | `.lt("price", 50)` |
| `.lte(col, val)` | `col <= val` | `.lte("stock", 0)` |
| `.like(col, pat)` | `col LIKE pat` | `.like("name", "Al%")` |
| `.ilike(col, pat)` | `col ILIKE pat` | `.ilike("email", "%@gmail%")` |
| `.in_values(col, vals)` | `col IN (...)` | `.in_values("id", ids)` |
| `.is_null(col)` | `col IS NULL` | `.is_null("deleted_at")` |
| `.is_not_null(col)` | `col IS NOT NULL` | `.is_not_null("email")` |

## Logical connectors

```rust
// AND (default)
User::filter(&pool)
    .eq("active", true)
    .eq("role", "admin")  // AND

// OR
User::filter(&pool)
    .eq("role", "admin")
    .or()
    .eq("role", "moderator")

// NOT
User::filter(&pool)
    .not()
    .eq("banned", true)

// Grouped OR
User::filter(&pool)
    .eq("active", true)
    .group(|q| q.eq("role", "admin").or().eq("role", "mod"))
// → WHERE "active" = $1 AND ("role" = $2 OR "role" = $3)
```

## Subqueries

Filter on another table without loading it. The closure gets an empty `filter()` on the other model and adds conditions to it; only those conditions are used (ordering, limit and relations are ignored). The foreign key column tells Rango which model the subquery runs on. Values stay bound parameters.

`where_exists` keeps rows that have at least one related row through a foreign key pointing at them:

```rust
// Users with at least one published article
let authors = User::filter(&pool)
    .where_exists(Article::AUTHOR_ID, |q| q.eq(Article::PUBLISHED, true))
    .all()
    .await?;
// WHERE EXISTS (SELECT 1 FROM "article"
//               WHERE "article"."author_id" = "user"."id" AND ("published" = $1))

// Users without any article
let idle = User::filter(&pool)
    .not()
    .where_exists(Article::AUTHOR_ID, |q| q)
    .all()
    .await?;
```

`in_subquery` compares a column with the values another query returns:

```rust
// Posts written by active users
let posts = Post::filter(&pool)
    .in_subquery(Post::AUTHOR_ID, User::ID, |q| q.eq(User::ACTIVE, true))
    .all()
    .await?;
// WHERE "author_id" IN (SELECT "id" FROM "user" WHERE "active" = $1)
```

Both compose with `.or()`, `.not()` and `.group()` like any other filter. For "has none", use `.not().where_exists(...)` rather than `.not().in_subquery(...)`: `NOT IN` matches nothing as soon as the subquery returns a `NULL`.

### When to use a subquery, a JOIN, or a prefetch

Rango does not pick between these for you — you express the intent, and each maps to the cheapest shape for that intent:

| You want… | Use | Why it is the cheap shape |
|---|---|---|
| Keep/drop rows based on **whether** a related row exists | `where_exists` (correlated `EXISTS`) | The DB stops scanning the inner table at the first matching row; no row multiplication, no `DISTINCT`, no `JOIN` to clean up |
| Compare a column against a **set of values** from another query | `in_subquery` (`IN (SELECT …)`) | One subquery, values stay bound; avoids loading them into Rust |
| **Select columns from** a related table alongside the row | `select_related` (single `JOIN`) | You need the related columns in the result, so a `JOIN` is the right tool — a subquery would drop them |
| **Load the related rows themselves** to iterate them | `prefetch_related` (one batched `IN` / `= ANY`) | One extra query for the whole page, which is what avoids the N+1 of a per-row lookup |

The rule of thumb: a subquery answers *"does it exist?"* or *"is it in this set?"* without touching the related rows; a `JOIN` or `prefetch` is only for when you actually need the related data in hand. Reaching for a `JOIN` just to filter (then `DISTINCT`-ing the duplicates) is the N+1-adjacent anti-pattern `where_exists` exists to replace.

## Ordering, limit, offset

```rust
User::filter(&pool)
    .order_by("name")       // ASC
    .order_by("-created_at") // DESC (prefix with -)
    .limit(20)
    .offset(40)
    .all()
    .await?
```

## Default limit

By default, `.all()` returns at most **1000 rows**. This is a safety cap, not a pagination boundary: when exactly 1000 rows come back and you set neither `.limit(n)` nor `.unlimited()`, Rango logs a `tracing` `WARN` naming the table, because rows may have been cut off.

Override with `.limit(n)` or remove with `.unlimited()`:

```rust
User::filter(&pool).limit(5000).all().await?  // explicit limit
User::filter(&pool).unlimited().all().await?   // no limit
```

The cap applies to the query builder only (`all()`, `values()`, `select_as::<P>().all()`). The plain `rango::all::<M>(&pool)` function has no cap and returns every row of the table.

## Terminal methods

| Method | Returns |
|---|---|
| `.all()` | `Vec<WithRelated<M>>` |
| `.one()` | `Option<M>` (no relation loading) |
| `.count()` | `i64` |
| `.exists()` | `bool` |
| `.explain()` | `String` — the SQL that would run |
| `.explain_analyze()` | `String` — the plan with real timings (runs the query) |

## Inspecting the SQL

```rust
let sql = User::filter(&pool)
    .eq("active", true)
    .order_by("name")
    .limit(10)
    .explain();

println!("{}", sql);
// SELECT * FROM "myapp_user" WHERE "active" = $1 ORDER BY "name" ASC LIMIT 10
```

A query builder takes `&pool` or `&mut **tx` (inside `atomic()`). `.explain_analyze().await?` (it takes `&mut self`, so the builder stays usable afterwards) runs the query under the engine's profiler and returns the plan:

| Backend | Statement | Executes the query |
|---|---|---|
| Postgres | `EXPLAIN ANALYZE` | yes |
| MySQL 8.0.18+ | `EXPLAIN ANALYZE` | yes |
| MariaDB | `ANALYZE FORMAT=JSON` | yes |
| SQLite | `EXPLAIN QUERY PLAN` (no timings) | no |

The rows are discarded, but the query does run: keep it for debugging, not hot paths.
