# Transactions

## atomic()

Wrap multiple operations in a transaction. Automatically commits on success, rolls back on error.

```rust
atomic(&pool, |tx| async move {
    let user = insert(tx, new_user).await?;
    let profile = insert(tx, Profile { user_id: ForeignKey::new(user.id.0), ..profile }).await?;
    Ok((user, profile))
})
.await?;
```

If any operation returns `Err`, the entire transaction is rolled back.

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
    .transaction(|tx| async move {
        insert(tx, user).await?;
        insert(tx, profile).await?;
        Ok(())
    })
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
