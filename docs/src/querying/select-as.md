# Partial selects (only / defer)
Select a subset of model columns into a partial struct.

## Declare a partial
```rust,ignore
#[derive(Partial)]
#[partial(model = User)]
struct UserLite {
    id: FieldUuid,
    email: FieldEmail,
}
```
Every field must match a model column by name and type. A wrong name or type is a compile error. `Partial` is in `rango::prelude`.

## Query
```rust,ignore
let users: Vec<UserLite> = User::filter(&pool)
    .eq(User::ACTIVE, true)
    .select_as::<UserLite>()
    .all()
    .await?;

let user: Option<UserLite> = User::filter(&pool)
    .eq(User::ACTIVE, true)
    .select_as::<UserLite>()
    .one()
    .await?;
```
The `SELECT` lists only the partial's columns, for example:
```sql
SELECT "id", "email" FROM "user" ...
```
Large columns such as bodies and password hashes are never read.

## Supported behavior
- Same filters, ordering, `limit`/`offset` as `.all()`.
- Relations (`select_related`, `prefetch_related`) are not supported with `select_as`.
- Works on Postgres, SQLite, MySQL.

## Why a struct?
Why a struct and not `.only(&["id"])` returning the model: a model with missing fields could be saved back with `update()` and overwrite columns. For untyped maps, `.values(&["id", "email"])` already exists (see [values](./values.md)).
