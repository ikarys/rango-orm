# Bulk Operations

Bulk operations insert or update multiple rows efficiently in a single query.

## bulk_create

Insert multiple rows in one query. Automatically chunks to stay within database parameter limits.

```rust
let users: Vec<User> = (0..1000)
    .map(|i| User {
        id: FieldUuid(uuid::Uuid::new_v4()),
        email: FieldEmail::new(format!("user{}@example.com", i))?,
        name: FieldVarchar::new(format!("User {}", i))?,
        active: FieldBool(true),
        created_at: FieldDateTime(chrono::Utc::now()),
    })
    .collect();

let inserted = bulk_create(&pool, &users).await?;
println!("Inserted {} rows", inserted);
```

On PostgreSQL, generates one `INSERT INTO ... SELECT * FROM UNNEST($1::type[], $2::type[], ...)` with one array per column, whatever the row count (falls back to multi-row `VALUES` for JSON columns). On SQLite and MySQL, generates one `INSERT INTO ... VALUES (...), (...), ...` per chunk.

## bulk_update

Update specific fields on multiple rows using a single query.

```rust
// Deactivate all users in the list
for user in &mut users {
    user.active = FieldBool(false);
}

let updated = bulk_update(&pool, &users, &["active"]).await?;
println!("Updated {} rows", updated);
```

Uses `UPDATE ... FROM (VALUES ...) AS v WHERE id = v.id`.

## bulk_upsert

Insert or update on conflict.

```rust
let updated = bulk_upsert(&pool, &users, &["email"]).await?;
// conflict_on: columns used to detect duplicates (must have a UNIQUE constraint)
```

Uses `INSERT ... ON CONFLICT (email) DO UPDATE SET ...`.

## Chunking

PostgreSQL and MySQL have a limit of 65,535 bind parameters per query. SQLite has a limit of 999.
Rango automatically splits large batches into chunks — transparent to the caller. Postgres `bulk_create` needs no chunking: UNNEST binds one array per column.

## Atomicity

Every bulk operation is all-or-nothing. When it needs several statements (chunked `VALUES`, `bulk_update`, `bulk_upsert`), Rango runs them in one transaction, so a failure in a later chunk (a duplicate, a constraint violation) leaves no earlier rows behind. Called inside your own transaction, it uses a savepoint.

## Inside transactions

`bulk_create` takes a pool or a transaction on all three backends (`&mut **tx` is the connection behind the transaction). `bulk_update` and `bulk_upsert` exist on PostgreSQL only.

```rust
atomic(&pool, |tx| Box::pin(async move {
    bulk_create(&mut **tx, &users).await?;
    bulk_create(&mut **tx, &posts).await?;
    Ok(())
})).await?;
```
