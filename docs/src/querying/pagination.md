# Pagination
## Offset pagination
`.limit(n)` / `.offset(n)` are simple, but `OFFSET` makes the database scan and discard skipped rows. It is slow on deep pages.

```rust,ignore
let page = User::filter(&pool).limit(100).offset(200).all().await?;
```

## Keyset pagination
`.after(col, value)` adds `col > value` and orders by `col ASC` first (no separate `.order_by(col)` needed; if you add one on the same column it is not duplicated).  
`.before(col, value)` adds `col < value` and orders by `col DESC` first.

Use a unique, indexed column, such as a primary key or unique timestamp. Cost is constant per page.

```rust,ignore
let mut last = None;
loop {
    let mut q = User::filter(&pool).order_by(User::ID).limit(100);
    if let Some(id) = last {
        q = q.after(User::ID, id);
    }
    let page = q.all().await?;
    let Some(row) = page.last() else { break };
    last = Some(row.id.clone());
}
```

Always call `.order_by(User::ID)` (the keyset column) on the first page too. `.after()` only puts its column first on the pages that call it. Without `order_by`, page 1 uses the model's default `ordering`, later pages use the keyset column, and rows are silently skipped.

## Default limit
When no limit is set, the default cap is 1000 rows, and a `WARN` is logged if exactly 1000 rows come back. Use `.unlimited()` to remove it.

```rust,ignore
let all = User::filter(&pool).unlimited().all().await?;
```

## Streaming
`.stream()` decodes the rows one at a time as they arrive from the connection, so memory stays constant whatever the count. There is no default cap: `.limit()` is honoured, nothing else bounds the rows. It returns a `RowStream` that implements `futures_core::Stream`; its inherent `next()` needs no combinator crate.

```rust,ignore
let mut rows = User::filter(&pool).eq(User::ACTIVE, true).stream()?;
while let Some(user) = rows.next().await {
    let user = user?;
    export(&user).await?;
}
```

The stream borrows the pool or the transaction the builder was made with, and holds a connection until it is dropped. `select_related` and `prefetch_related` are not loaded by `.stream()`: they return `Error::Invalid`, use `.all()` for those.
