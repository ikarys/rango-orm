# Constraints & Meta

## Check constraints

```rust
use rango::prelude::*;

#[derive(Model)]
#[model(constraints = [
    CheckConstraint::raw("age >= 0").name("chk_age_positive"),
    CheckConstraint::raw("price > 0").name("chk_price_positive"),
])]
pub struct Product {
    pub id: FieldUuid,
    pub name: FieldVarchar<1, 255>,
    pub price: FieldDecimal<10, 2>,
    pub age_restriction: FieldInt,
}
```

Generates:
```sql
CREATE TABLE "myapp_product" (
    "id" UUID PRIMARY KEY,
    ...
    CONSTRAINT "chk_age_positive" CHECK (age >= 0),
    CONSTRAINT "chk_price_positive" CHECK (price > 0)
);
```

## Unique constraints

```rust
#[model(constraints = [
    // Simple unique
    UniqueConstraint::on(&["email"]).name("uniq_email"),

    // Composite unique
    UniqueConstraint::on(&["room", "date"]).name("uniq_booking"),

    // Partial unique (Postgres only)
    UniqueConstraint::on(&["email"])
        .condition("deleted_at IS NULL")
        .name("uniq_active_email"),
])]
```

## Field-level unique

For single-column uniqueness, `#[field(unique)]` is simpler:

```rust
#[field(unique)]
pub email: FieldEmail,
```

## Default ordering

```rust
#[model(ordering = [asc("name"), desc("created_at")])]
pub struct Article { ... }
```

This ordering is injected automatically into queries on this model.

## Unmanaged models

Mark a model as `managed = false` to exclude it from migrations.
Useful for mapping external tables or views:

```rust
#[model(managed = false, table = "external_users")]
pub struct ExternalUser {
    pub id: FieldUuid,
    pub email: FieldEmail,
}
```

Rango won't create or modify this table — you manage it yourself.
