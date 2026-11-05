# Defining Models

A Rango model is a regular Rust struct annotated with `#[derive(Model)]`.

```rust
use rango::prelude::*;

#[derive(Model)]
pub struct Article {
    pub id: FieldUuid,
    pub title: FieldVarchar<1, 255>,
    pub body: FieldText,
    pub published: FieldBool,
    pub created_at: FieldDateTime,
}
```

## Table name

By default, the table name is the snake_case of the struct name, prefixed with your package name.

`Article` → `myapp_article`

Override with `#[model(table = "...")]`:

```rust
#[derive(Model)]
#[model(table = "cms_articles")]
pub struct Article { ... }
```

## Primary key

Declare a field named `id` — it becomes the primary key automatically:

```rust
pub id: FieldUuid,      // UUID primary key (recommended)
pub id: FieldBigInt,    // Auto-increment integer
```

Or use `#[field(primary_key)]` for a custom name:

```rust
#[field(primary_key)]
pub uuid: FieldUuid,
```

## Nullable fields

Use `Option<T>` for nullable columns:

```rust
pub bio: Option<FieldText>,       // NULL allowed
pub score: Option<FieldInt>,      // NULL allowed
pub name: FieldVarchar<1, 100>,   // NOT NULL
```

## Field attributes

```rust
#[field(unique)]                  // UNIQUE constraint
#[field(index)]                   // creates an index
#[field(column = "custom_name")]  // override column name
#[field(primary_key)]             // mark as primary key
#[field(auto_now_add)]            // set to now() on INSERT (FieldDateTime only)
#[field(auto_now)]                // update to now() on every UPDATE
#[field(default = "0")]           // SQL default value
```

## Model attributes

```rust
#[model(table = "custom_name")]
#[model(managed = false)]          // exclude from migrations (external table)
#[model(ordering = [asc("name"), desc("created_at")])]
#[model(constraints = [
    CheckConstraint::raw("age >= 0").name("chk_age"),
    UniqueConstraint::on(&["email", "tenant_id"]).name("uniq_email_tenant"),
])]
```
