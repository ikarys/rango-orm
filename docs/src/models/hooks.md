# Model Hooks

Hooks let you run custom logic before or after single-row database operations.

## Implementing hooks

Hooks are opt-in. `#[derive(Model)]` emits an empty `impl ModelHooks` for you; add
`#[model(hooks)]` to write your own instead.

```rust
use rango::prelude::*;

#[derive(Model)]
#[model(hooks)]
pub struct User {
    pub id: FieldUuid,
    pub email: FieldEmail,
    pub slug: FieldVarchar<0, 100>,
}

impl ModelHooks for User {
    fn before_save(&mut self) -> BoxFuture<'_> {
        Box::pin(async move {
            // Runs before every INSERT or UPDATE; the mutation is what gets written.
            self.slug = FieldVarchar::new(self.email.replace('@', "-at-"))?;
            Ok(())
        })
    }

    fn after_create(&self) -> BoxFuture<'_> {
        Box::pin(async move {
            println!("Welcome {}!", self.email);
            Ok(())
        })
    }
}
```

`#[rango::model(...)]` forwards the attribute: write `#[model(hooks)]` on the struct as well.

## Available hooks

| Hook | Triggered |
|---|---|
| `before_save` | Before INSERT or UPDATE |
| `after_save` | After INSERT or UPDATE |
| `before_create` | Before INSERT only |
| `after_create` | After INSERT only |
| `before_update` | Before UPDATE only |
| `after_update` | After UPDATE only |
| `before_delete` | Before DELETE |
| `after_delete` | After DELETE |

## Which operations fire hooks

| Operation | Order |
|---|---|
| `insert` | `before_save`, `before_create`, SQL, `after_create`, `after_save` |
| `update` | `before_save`, `before_update`, SQL, `after_update`, `after_save` |
| `delete` | `before_delete`, SQL, `after_delete` |
| `get_or_create` | create hooks on `defaults`: `before_*` always run before the statement, `after_*` only if a row was inserted |
| `update_or_create` | create hooks or update hooks, depending on whether the row existed |

> **Bulk operations skip hooks.** `bulk_create`, `bulk_update` and `bulk_upsert` never call
> any hook (see [Bulk operations](../operations/bulk.md)). If a hook computes a field, such as
> the `slug` above, compute it yourself before a bulk call or insert row by row.

Not fired (same as Django): query-builder `update` / `delete` and `raw*`, as well as the bulk operations above.

`before_*` hooks take `&mut self`, so they can change the model; `after_*` hooks see the row
returned by the database.

## Errors

- All hooks are `async` and return `Result<(), Box<dyn Error + Send + Sync>>`.
- A `before_*` hook returning `Err` cancels the operation: no SQL runs. The caller gets `Error::Hook`.
- An `after_*` hook returning `Err` is returned to the caller as `Error::Hook` too, but the
  statement has already run. Inside `atomic()` (or any transaction) the error rolls the
  write back. With a plain pool the write is already committed: use `atomic()` if an `after_*`
  hook must be able to undo it.
- Hooks never open a transaction by themselves.

```rust
impl ModelHooks for User {
    fn before_save(&mut self) -> BoxFuture<'_> {
        Box::pin(async move {
            if self.email.is_empty() {
                return Err("email cannot be empty".into());
            }
            Ok(())
        })
    }
}
```
