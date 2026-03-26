# Bulk Operations

Bulk operations insert or update multiple rows efficiently in a single query.

## bulk_create

Insert multiple rows in one query. Automatically chunks to stay within database parameter limits.

```rust
let users: Vec<User> = (0..1000)
    .map(|i| User {
        id: FieldUuid(uuid::Uuid::new_v4()),
        email: FieldEmail(format!("user{}@example.com", i)),
        name: FieldVarchar(format!("User {}", i)),
        active: FieldBool(true),
        created_at: FieldDateTime(chrono::Utc::now()),
    })
    .collect();

let inserted = bulk_create(&pool, &users).await?;
println!("Inserted {} rows", inserted);
```

Generates a single `INSERT INTO ... VALUES (...), (...), ...` per chunk.

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

PostgreSQL has a limit of 65,535 bind parameters per query. SQLite has a limit of 999.
Rango automatically splits large batches into chunks — transparent to the caller.

## Inside transactions

All bulk ops accept a transaction executor:

```rust
atomic(&pool, |tx| async move {
    bulk_create(tx, &users).await?;
    bulk_create(tx, &posts).await?;
    Ok(())
}).await?;
```
