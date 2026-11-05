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
// → SELECT * FROM "myapp_user" WHERE "email" = $1
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

## Column names are not escaped

Filter column names (e.g. `.eq("email", ...)`) are not user-controlled — they're hardcoded in your source. They are quoted but not parameterized.

```rust
// Column "email" is quoted as "email" in SQL, but it comes from your code, not user input
User::filter(&pool).eq("email", user_input)
```

**Never pass user-controlled strings as column names.**

## Summary

| Path | Safe? |
|---|---|
| Query builder (`.eq`, `.gt`, etc.) | ✅ Always safe |
| `raw()` with bound params | ✅ Safe |
| `raw()` with string interpolation | ❌ Never do this |
| Column names from user input | ❌ Never do this |
