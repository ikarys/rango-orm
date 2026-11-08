# SQL Injection

## Safe by default

Every Rango query builder operation uses prepared statements with bound parameters.
Values are never interpolated into SQL strings.

```rust
// Safe — email is bound as $1, never interpolated
User::filter(&pool)
    .eq("email", user_input)
    .all()
    .await?;
// → SELECT * FROM "user" WHERE "email" = $1
```

The database driver handles all escaping. SQL injection via the query builder is impossible.

## Raw SQL is your responsibility

When using `raw()`, `raw_scalar()`, or `raw_execute()`, you write the SQL yourself.
Rango cannot protect you from injection in the SQL string itself.

```rust
// ❌ DANGEROUS
let sql = format!("SELECT * FROM users WHERE name = '{}'", user_input);
raw_execute(&pool, &sql, vec![]).await?;

// ✅ SAFE — always use bound parameters
raw_execute(
    &pool,
    "SELECT * FROM users WHERE name = $1",
    vec![SqlValue::Text(user_input)],
).await?;
```

## Column names

Prefer typed columns (`User::EMAIL`): they are checked at compile time. A column name passed as a string (`.eq("email", ...)`, `.order_by("created_at")`) is checked against the model's schema before any SQL runs, an unknown name returns `Error::Invalid`, and the name is always quoted with `quote_ident`. A hostile string cannot inject SQL.

It can still pick any column of the model. If a request chooses a sort or filter column (`?sort=...`), map it to an allowed list first, so a client cannot filter or sort on a column it should not see (a password hash, an internal flag).

```rust
let col = match params.sort.as_str() {
    "name" => User::NAME.asc(),
    "created" => User::CREATED_AT.desc(),
    _ => User::CREATED_AT.desc(),
};
User::filter(&pool).order_by(col)
```

## Summary

| Path | Safe? |
|---|---|
| Query builder (`.eq`, `.gt`, etc.) | ✅ Always safe |
| `raw()` with bound params | ✅ Safe |
| `raw()` with string interpolation | ❌ Never do this |
| String column names | ✅ Validated against the schema and quoted; restrict user-chosen columns to an allowed list |

Every claim on this page and the test behind it: [Security requirements](../trust/security-requirements.md).
