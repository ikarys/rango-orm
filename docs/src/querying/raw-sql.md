# Raw SQL

Raw SQL is an escape hatch for queries the builder can't express.
**Always use bound parameters — never interpolate user input.**

## raw() — returns model rows

```rust
let users = raw::<User>(
    &pool,
    "SELECT * FROM myapp_user WHERE role = $1 AND created_at > $2",
    vec![
        SqlValue::Text("admin".into()),
        SqlValue::DateTime(cutoff_date),
    ],
).await?;
```

## raw_scalar() — returns a single value

```rust
let count: i64 = raw_scalar(
    &pool,
    "SELECT COUNT(*) FROM myapp_user WHERE active = $1",
    vec![SqlValue::Bool(true)],
).await?;

let name: String = raw_scalar(
    &pool,
    "SELECT name FROM myapp_user WHERE id = $1",
    vec![SqlValue::Uuid(user_id)],
).await?;
```

## raw_execute() — INSERT / UPDATE / DELETE / DDL

```rust
let affected = raw_execute(
    &pool,
    "UPDATE myapp_user SET score = score + $1 WHERE active = $2",
    vec![SqlValue::Int(10), SqlValue::Bool(true)],
).await?;

println!("{} users updated", affected);
```

## ⚠️ Security

`raw()` does **not** escape your SQL string. Only the bound parameters are safe.

```rust
// ❌ NEVER DO THIS
let sql = format!("SELECT * FROM users WHERE name = '{}'", user_input);
raw_execute(&pool, &sql, vec![]).await?;

// ✅ ALWAYS use bound parameters
raw_execute(
    &pool,
    "SELECT * FROM users WHERE name = $1",
    vec![SqlValue::Text(user_input)],
).await?;
```

## Placeholders

| Backend | Placeholder |
|---|---|
| PostgreSQL | `$1`, `$2`, `$3`... |
| SQLite | `?`, `?`, `?`... |

## When to use raw SQL

Use `raw()` when:
- Complex JOINs across multiple tables
- Window functions (`ROW_NUMBER`, `RANK`, etc.)
- Database-specific functions not in the query builder
- Performance-critical queries needing precise SQL

If you find yourself using `raw()` often, consider opening an issue — it might be a missing feature in the query builder.
