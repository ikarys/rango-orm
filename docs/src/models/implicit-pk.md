# Implicit primary key
`#[derive(Model)]` requires a primary key. The attribute macro `#[rango::model]` adds one when missing, then derives `Model`.

## Behavior
- If no primary key is declared, it injects `pub id: FieldUuid` as the first field.
- `#[rango::model(pk = "bigint")]` injects `id: FieldAutoBigInt`, assigned by the database on insert.
- `#[rango::model(pk = "int")]` injects `id: FieldAutoInt`.
- `"uuid"` is the default.
- If a primary key already exists (`id` field or `#[field(primary_key)]`), nothing is injected.
- `#[derive(Model)]` is added unless already present, so the struct can still carry `#[model(...)]` and `#[field(...)]` attributes.

Use `#[rango::model]` as written. It is not imported in the prelude, on purpose, to avoid clashing with other `model` names.

The default is set per model, not in `rango.toml`. Cargo does not track that file, so builds would go stale when it changes.

## Example
```rust,ignore
#[rango::model]
struct Article {
    title: FieldVarchar<1, 200>,
    body: FieldText,
}
// Article now has `pub id: FieldUuid`.

#[rango::model(pk = "bigint")]
struct Event {
    name: FieldVarchar<1, 100>,
}
// Event has `id: FieldAutoBigInt`, assigned by the database on insert.
```
