# Model Hooks

Hooks let you run custom logic before or after database operations.

## Implementing hooks

```rust
use rango::prelude::*;

#[derive(Model)]
pub struct User {
    pub id: FieldUuid,
    pub email: FieldEmail,
    pub slug: FieldVarchar<1, 100>,
    pub password_hash: FieldText,
}

impl ModelHooks for User {
    async fn before_save(&mut self) -> anyhow::Result<()> {
        // Generate slug from email before every INSERT or UPDATE
        self.slug = FieldVarchar(self.email.0.replace('@', "-at-"));
        Ok(())
    }

    async fn after_create(&self) -> anyhow::Result<()> {
        // Send welcome email after first INSERT
        println!("Welcome {}!", self.email.0);
        Ok(())
    }
}
```

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

## Rules

- All hooks are `async`
- Returning `Err(...)` **cancels** the operation and rolls back the transaction
- Hooks run inside the same transaction as the DB operation
- `ModelHooks` is opt-in — no impl needed if unused

## Cancelling an operation

```rust
impl ModelHooks for User {
    async fn before_save(&mut self) -> anyhow::Result<()> {
        if self.email.0.is_empty() {
            anyhow::bail!("Email cannot be empty");
        }
        Ok(())
    }
}
```

The `before_save` error will propagate back to the caller — no row is written.
