# Transactions

## atomic()

Wrap multiple operations in a transaction. Automatically commits on success, rolls back on error.

```rust
atomic(&pool, async |tx| {
    let user = insert(&mut **tx, new_user).await?;
    let profile = insert(&mut **tx, Profile { user_id: ForeignKey::new(user.id.0), ..profile }).await?;
    Ok((user, profile))
})
.await?;
```

The body is an async closure (`async |tx| { ... }`, Rust 1.85+). The future it returns is `Send`, so `tokio::spawn(async move { atomic(&pool, async |tx| { ... }).await })` works.

If any operation returns `Err`, the entire transaction is rolled back. To abort on a business rule, return `Err(Error::abort("insufficient funds"))`.

## Retrying on conflict

Use `atomic_retry()` for `SERIALIZABLE` / `REPEATABLE READ` workloads and deadlocks. It retries only `Error::Conflict` (Postgres: serialization failure `40001`, deadlock `40P01`; MySQL/MariaDB: deadlock 1213 and lock wait timeout 1205); other errors are returned immediately.

```rust
atomic_retry(&pool, 3, async |tx| {
    raw_execute(&mut **tx, "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE", vec![]).await?;
    // ...
    Ok(())
})
.await?;
```

The closure can run several times, so keep it free of side effects outside the transaction (no emails, no HTTP calls). Postgres and MySQL/MariaDB (`atomic()` works on every backend, SQLite has no equivalent). On MySQL a lock wait timeout is a `Conflict` (so it is retried), while on Postgres `lock_timeout` stays `Error::Timeout`.

## Manual transactions

For more control, use `pool.begin()` directly:

```rust
let mut tx = pool.begin().await?;

insert(&mut *tx, user).await?;
insert(&mut *tx, profile).await?;

tx.commit().await?;
// or tx.rollback().await? to cancel
```

## Queries, M2M and bulk inside a transaction

`filter`, the `M2M` operations and the bulk operations take a pool or a transaction, on every backend. Inside `atomic()` pass `&mut **tx`:

```rust
atomic(&pool, async |tx| {
    bulk_create(&mut **tx, &users).await?;
    let n = User::filter(&mut **tx).eq("active", true).count().await?; // sees the writes above
    M2M::<Article, Tag>::add(&mut **tx, &article, &tag).await?;
    Ok(n)
})
.await?;
```

If the closure returns `Err`, none of it is kept.

## Nesting

Transactions can be nested — inner failures roll back to the outer savepoint.
This is handled automatically by the database driver.

## Hooks inside transactions

Model hooks run inside the same transaction as the operation.
If a hook returns `Err`, the whole transaction rolls back — including any operations that ran before the hook.

## get_or_create is atomic

`get_or_create` uses `INSERT ... ON CONFLICT DO NOTHING RETURNING *` (SQLite, MySQL: the equivalent for the engine) — it's atomic without wrapping in `atomic()`. Safe against race conditions. It takes a pool or a transaction.
