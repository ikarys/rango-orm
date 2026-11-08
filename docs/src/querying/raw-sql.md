# Raw SQL

Raw SQL is an escape hatch for queries the builder can't express.
**Always use bound parameters — never interpolate user input.**

Table names in the examples are `app_user` (a model with `#[model(table = "app_user")]`). Quote reserved names such as `user` or `order`: `"user"` on PostgreSQL and SQLite, `` `user` `` on MySQL. On PostgreSQL an unquoted `user` is the `current_user` function, so `SELECT * FROM user` does not read your table.

> **Imports.** The examples call `raw`, `raw_scalar`, `raw_execute` (and elsewhere `insert`, `bulk_create`, ...) as plain functions. With `use rango::prelude::*;` they are in scope on any backend. At the crate root (`rango::raw_execute`, `rango::bulk_create`) they exist only with the `postgres` feature. With only `sqlite` or `mysql`, write `rango::sqlite::raw_execute` / `rango::mysql::raw_execute`, or use the prelude.

## raw() — returns model rows

`raw` takes the executor and the row type: `raw::<_, User>(...)`, or let the binding infer it with `let users: Vec<User> = raw(...)`.

```rust
let users = raw::<_, User>(
    &pool,
    "SELECT * FROM app_user WHERE role = $1 AND created_at > $2",
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
    "SELECT COUNT(*) FROM app_user WHERE active = $1",
    vec![SqlValue::Bool(true)],
).await?;

let name: String = raw_scalar(
    &pool,
    "SELECT name FROM app_user WHERE id = $1",
    vec![SqlValue::Uuid(user_id)],
).await?;
```

## raw_execute() — INSERT / UPDATE / DELETE / DDL

```rust
let affected = raw_execute(
    &pool,
    "UPDATE app_user SET score = score + $1 WHERE active = $2",
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
let users = raw::<_, User>(&pool, "SELECT * FROM active_users($1)", vec![SqlValue::Int(30)]).await?;

// MySQL / MariaDB: the procedure's result set comes back like a SELECT
let rows = raw::<_, User>(&pool, "CALL active_users(?)", vec![SqlValue::Int(30)]).await?;
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
