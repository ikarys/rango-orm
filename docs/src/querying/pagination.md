# Pagination
## Offset pagination
`.limit(n)` / `.offset(n)` are simple, but `OFFSET` makes the database scan and discard skipped rows. It is slow on deep pages.

```rust,ignore
let page = User::filter(&pool).limit(100).offset(200).all().await?;
```

## Keyset pagination
`.after(col, value)` adds `col > value` and orders by `col ASC` first.  
`.before(col, value)` adds `col < value` and orders by `col DESC` first.

Use a unique, indexed column, such as a primary key or unique timestamp. Cost is constant per page.

```rust,ignore
let mut last = None;
loop {
    let mut q = User::filter(&pool).limit(100);
    if let Some(id) = last {
        q = q.after(User::ID, id);
    }
    let page = q.order_by(User::ID).all().await?;
    let Some(row) = page.last() else { break };
    last = Some(row.id.clone());
}
```

## Default limit
When no limit is set, the default cap is 1000 rows, and a `WARN` is logged if exactly 1000 rows come back. Use `.unlimited()` to remove it.

```rust,ignore
let all = User::filter(&pool).unlimited().all().await?;
```
