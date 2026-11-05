# CRUD

## Insert

```rust
use rango::prelude::*;

let user = User {
    id: FieldUuid(uuid::Uuid::new_v4()),
    email: FieldEmail("alice@example.com".into()),
    name: FieldVarchar("Alice".into()),
    active: FieldBool(true),
    created_at: FieldDateTime(chrono::Utc::now()),
};

let inserted = insert(&pool, user).await?;
// Returns the saved model with any DB-generated values
```

## Get by primary key

```rust
let user = get::<_, User>(&pool, &SqlValue::Uuid(user_id)).await?;

match user {
    Some(u) => println!("Found: {}", u.email.0),
    None    => println!("Not found"),
}
```

## Update

```rust
let mut user = get::<_, User>(&pool, &id).await?.unwrap();
user.name = FieldVarchar("Alice Updated".into());
let updated = update(&pool, user).await?;
```

## Delete

```rust
delete(&pool, &user).await?;
```

## get_or_create

Atomic — safe against race conditions:

```rust
let lookup = vec![("email", SqlValue::Text("alice@example.com".into()))];
let defaults = User { /* ... */ };

let (user, created) = get_or_create(&pool, lookup, defaults).await?;

if created {
    println!("New user created");
} else {
    println!("Existing user found");
}
```

Uses `INSERT ... ON CONFLICT DO NOTHING RETURNING *` in a single round-trip.

## update_or_create

```rust
let lookup = vec![("email", SqlValue::Text("alice@example.com".into()))];
let values = User { /* ... */ };

let user = update_or_create(&mut pool, lookup, values).await?;
```

## all()

Fetch all rows (subject to the [default limit](../querying/filter.md#default-limit)):

```rust
let users = all::<_, User>(&pool).await?;
```
