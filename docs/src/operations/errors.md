# Errors

Every operation returns `rango::Result<T>`, with a typed `rango::Error`. Match on variants instead of parsing strings:

```rust
match insert(&pool, user).await {
    Ok(user) => Ok(user),
    Err(Error::UniqueViolation { .. }) => Err(ApiError::Conflict("email already taken")),
    Err(e) if e.is_retryable() => retry(),
    Err(e) => Err(e.into()),
}
```

| Variant | When |
|---|---|
| `NotFound` | A query expected a row and found none |
| `TableMissing { table }` | The table does not exist: run `rango makemigrations` and `rango migrate` |
| `UniqueViolation` / `ForeignKeyViolation` / `NotNullViolation` / `CheckViolation` | Constraint violated; `constraint` holds its name when the backend reports it (on SQLite, the failing columns, e.g. `user.email`; on MySQL and MariaDB, parsed from the server message, without the `table.` prefix MySQL 8 adds to unique keys; never the value or the SQL; `NotNullViolation` carries none) |
| `Conflict` | Serialization failure or deadlock (`40001` / `40P01`); `is_retryable()` is true |
| `Timeout` | Pool acquire, `statement_timeout`, `lock_timeout`, or SQLite busy timeout |
| `Connection` | IO, TLS, or closed pool |
| `Decode` | A row could not be decoded into the model |
| `Invalid` | Invalid input or configuration; an unknown column name suggests the closest one (`did you mean `email`?`) |
| `Database { code, message }` | Any other database error (`code` is the SQLSTATE on Postgres) |
| `Aborted` | Your code returned `Error::abort(..)` inside `atomic()`; the transaction was rolled back |

Errors never contain SQL text or bound values. The `Database` message is the database's own message, which can quote an offending value (e.g. invalid input syntax). Log it, but don't send it to clients as-is.

`rango::Error` implements `std::error::Error + Send + Sync`, so `?` converts it into `anyhow::Error` or your own error type.
