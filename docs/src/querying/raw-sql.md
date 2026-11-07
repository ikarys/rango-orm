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

## Stored procedures and functions

There is no `rango::call`: a procedure call is one line of SQL, and the `raw*` functions already bind its arguments.

```rust
// PostgreSQL procedure: CALL returns the INOUT parameters as one row
let total: i32 = raw_scalar(&pool, "CALL compute_total($1, NULL)", vec![SqlValue::Int(order_id)]).await?;

// PostgreSQL function returning a set: select from it like a table
let users = raw::<User>(&pool, "SELECT * FROM active_users($1)", vec![SqlValue::Int(30)]).await?;

// MySQL / MariaDB: the procedure's result set comes back like a SELECT
let rows = raw::<User>(&pool, "CALL active_users(?)", vec![SqlValue::Int(30)]).await?;
```

The procedure name is part of the SQL string: write it in the code, never take it from user input. SQLite has no stored procedures.

Create procedures in a migration file, not with `raw_execute`: MySQL 8.4 refuses `CREATE PROCEDURE` as a prepared statement, and `rango migrate` sends migration files as plain text.

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
| MySQL / MariaDB | `?`, `?`, `?`... |
| SQLite | `?`, `?`, `?`... |

## When to use raw SQL

Use `raw()` when:
- Complex JOINs across multiple tables
- Window functions (`ROW_NUMBER`, `RANK`, etc.)
- Database-specific functions not in the query builder
- Performance-critical queries needing precise SQL

If you find yourself using `raw()` often, consider opening an issue — it might be a missing feature in the query builder.
