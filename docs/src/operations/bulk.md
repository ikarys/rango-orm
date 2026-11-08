# Bulk Operations

Bulk operations insert or update multiple rows efficiently in a single query.

Bulk operations do not run [model hooks](../models/hooks.md) (`before_save`, `after_create`, ...).

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

Update specific fields on multiple rows with one statement per chunk.

```rust
// Deactivate all users in the list
for user in &mut users {
    user.active = FieldBool(false);
}

let updated = bulk_update(&pool, &users, &["active"]).await?;
println!("Updated {} rows", updated);
```

On PostgreSQL this is `UPDATE ... FROM (VALUES ...) AS v WHERE id = v.id`. SQLite uses `WITH v(id, ..) AS (VALUES ..) UPDATE ... FROM v WHERE t.id = v.id`, and MySQL / MariaDB use `UPDATE ... SET col = CASE id WHEN ? THEN ? ... END WHERE id IN (...)` (a derived table is typed differently by the two engines). That is one statement per chunk on every backend; several chunks run in one transaction. MySQL chunks are capped at 100 rows because a `CASE` is scanned linearly for each row. Unknown columns and the primary key are rejected with `Error::Invalid` before any SQL runs.

## bulk_upsert

Insert or update on conflict.

```rust
let updated = bulk_upsert(&pool, &users, &["email"]).await?;
// conflict_on: columns used to detect duplicates (must have a UNIQUE constraint)
```

The return value is the number of rows the database reports as affected, summed over chunks. On PostgreSQL and SQLite it is one per inserted or updated row. On MySQL / MariaDB it is 1 per inserted row and 2 per updated row (and 0 for a row left unchanged), so it is not the number of models you passed.

Uses `INSERT ... ON CONFLICT (email) DO UPDATE SET ...` on PostgreSQL and SQLite, and `INSERT ... ON DUPLICATE KEY UPDATE` on MySQL. MySQL fires on a duplicate of any unique index and cannot target one, so Rango checks the model first: if it has a unique key other than `conflict_on` (a `#[field(unique)]` column or a non-partial `UniqueConstraint`, compared as an unordered column set), `bulk_upsert` returns `Error::Invalid` before any SQL runs. The primary key is exempt (a PK collision is the same row), but upserting on the primary key is rejected when another unique key exists. On MySQL the `conflict_on` columns are not overwritten.

## Chunking

PostgreSQL and MySQL have a limit of 65,535 bind parameters per query. SQLite (3.32+, bundled with sqlx) allows 32,766; Rango chunks at 32,000.
Rango automatically splits large batches into chunks — transparent to the caller. Postgres `bulk_create` needs no chunking: UNNEST binds one array per column.

## Atomicity

Every bulk operation is all-or-nothing. When it needs several statements (chunked `VALUES`, `bulk_update`, `bulk_upsert`), Rango runs them in one transaction, so a failure in a later chunk (a duplicate, a constraint violation) leaves no earlier rows behind. Called inside your own transaction, it uses a savepoint.

## Inside transactions

`bulk_create`, `bulk_update` and `bulk_upsert` take a pool or a transaction on all three backends (`&mut **tx` is the connection behind the transaction).

```rust
atomic(&pool, async |tx| {
    bulk_create(&mut **tx, &users).await?;
    bulk_create(&mut **tx, &posts).await?;
    bulk_update(&mut **tx, &users, &["active"]).await?;
    Ok(())
}).await?;
```
