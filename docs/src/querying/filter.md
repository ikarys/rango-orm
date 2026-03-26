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

By default, `.all()` returns at most **1000 rows**.

Override with `.limit(n)` or remove with `.unlimited()`:

```rust
User::filter(&pool).limit(5000).all().await?  // explicit limit
User::filter(&pool).unlimited().all().await?   // no limit
```

## Terminal methods

| Method | Returns |
|---|---|
| `.all()` | `Vec<WithRelated<M>>` |
| `.one()` | `Option<WithRelated<M>>` |
| `.count()` | `i64` |
| `.exists()` | `bool` |
| `.explain()` | `String` — the SQL that would run |

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
