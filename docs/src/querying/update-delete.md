# Update & Delete

The query builder writes too. Filter, then `.set(..).update()` or `.delete()`: one statement, run by the database, no row loaded in Rust.

## update

```rust
let n = User::filter(&pool)
    .lt(User::AGE, 18)
    .set(User::ACTIVE, false)
    .set("note", "minor")
    .update()
    .await?; // u64 rows affected
// → UPDATE "user" SET "active" = $1, "note" = $2 WHERE "age" < $3
```

`set(col, value)` is typed like `eq`: a typed column (`User::ACTIVE`) is checked at compile time, a string name against the model at run time (`Error::Invalid`). Call it as many times as needed; `update()` without a `set` is `Error::Invalid`.

## delete

```rust
let n = Session::filter(&pool)
    .lt(Session::EXPIRES_AT, now)
    .delete()
    .await?;
// → DELETE FROM "session" WHERE "expires_at" < $1
```

A builder with no filter deletes every row (Django semantics):

```rust
Session::filter(&pool).delete().await?; // DELETE FROM "session"
```

`ON DELETE` actions declared on foreign keys run in the database, in the same statement.

## Rules

- Both return the number of rows affected, as reported by the driver.
- A filter only: `order_by`, `limit`, `offset`, `select_related` and `prefetch_related` are rejected with `Error::Invalid` before any SQL runs. The default 1000-row cap of `.all()` does not apply to writes.
- Model hooks (`before_update`, `after_delete`, ...) are **not** fired, like `bulk_*` and raw SQL. Use `update(&pool, &mut model)` / `delete(&pool, &model)` on a single row when a hook must run.
- Pool or transaction: `User::filter(&mut **tx)` inside `atomic()` works like for reads.

## When to use what

| Need | Use |
|---|---|
| Same value on many rows matching a condition | `filter().set().update()` |
| Different values per row, rows already in memory | `bulk_update(&pool, &models, &[cols])` |
| One row, hooks must run | `update(&pool, &mut model)` / `delete(&pool, &model)` |
| Remove rows by condition | `filter().delete()` |
