# Transactions

## atomic()

Wrap multiple operations in a transaction. Automatically commits on success, rolls back on error.

```rust
atomic(&pool, |tx| Box::pin(async move {
    let user = insert(&mut **tx, new_user).await?;
    let profile = insert(&mut **tx, Profile { user_id: ForeignKey::new(user.id.0), ..profile }).await?;
    Ok((user, profile))
}))
.await?;
```

The body is wrapped in `Box::pin` because the future borrows `tx` (same pattern as sqlx's `Connection::transaction`).

If any operation returns `Err`, the entire transaction is rolled back. To abort on a business rule, return `Err(Error::abort("insufficient funds"))`.

## Retrying on conflict

Use `atomic_retry()` for `SERIALIZABLE` / `REPEATABLE READ` workloads and deadlocks. It retries only `Error::Conflict`; other errors are returned immediately.

```rust
atomic_retry(&pool, 3, |tx| Box::pin(async move {
    raw_execute(&mut **tx, "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE", vec![]).await?;
    // ...
    Ok(())
}))
.await?;
```

The closure can run several times, so keep it free of side effects outside the transaction (no emails, no HTTP calls). Postgres only.

## Manual transactions

For more control, use `pool.begin()` directly:

```rust
let mut tx = pool.begin().await?;

insert(&mut *tx, user).await?;
insert(&mut *tx, profile).await?;

tx.commit().await?;
// or tx.rollback().await? to cancel
```

## QueryBuilder.transaction()

Convenience method when you already have a `QueryBuilder` in scope:

```rust
User::filter(&pool)
    .transaction(|tx| Box::pin(async move {
        insert(&mut **tx, user).await?;
        insert(&mut **tx, profile).await?;
        Ok(())
    }))
    .await?;
```

## Nesting

Transactions can be nested — inner failures roll back to the outer savepoint.
This is handled automatically by the database driver.

## Hooks inside transactions

Model hooks run inside the same transaction as the operation.
If a hook returns `Err`, the whole transaction rolls back — including any operations that ran before the hook.

## get_or_create is atomic

`get_or_create` uses `INSERT ... ON CONFLICT DO NOTHING RETURNING *` — it's atomic without wrapping in `atomic()`. Safe against race conditions.
