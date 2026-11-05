# values()

Load only specific columns without deserializing a full model.
Returns `Vec<HashMap<String, SqlValue>>`.

## Usage

```rust
let rows = User::filter(&pool)
    .eq("active", true)
    .values(&["id", "email"])
    .await?;

for row in &rows {
    if let Some(SqlValue::Text(email)) = row.get("email") {
        println!("{}", email);
    }
}
```

## Why use values()?

- Avoid loading heavy columns (e.g. large `TEXT` fields) when you only need a few
- Build lightweight projections for APIs
- Aggregate data without full model deserialization

## SqlValue variants

| Postgres type | SqlValue |
|---|---|
| `BOOLEAN` | `SqlValue::Bool(bool)` |
| `INT2/INT4/INT8` | `SqlValue::BigInt(i64)` |
| `FLOAT4/FLOAT8` | `SqlValue::Double(f64)` |
| `UUID` | `SqlValue::Uuid(Uuid)` |
| `TEXT/VARCHAR/...` | `SqlValue::Text(String)` |
| `NULL` | `SqlValue::NullText` (or typed null) |

## With filters and ordering

All QueryBuilder methods work before `.values()`:

```rust
let rows = Post::filter(&pool)
    .eq("published", true)
    .order_by("-created_at")
    .limit(10)
    .values(&["id", "title", "created_at"])
    .await?;
```

## Difference from raw SQL

`values()` still uses the query builder's WHERE conditions and is safe from SQL injection.
Use `raw()` only when you need SQL that the query builder can't express.
