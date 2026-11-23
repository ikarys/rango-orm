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
| `NUMERIC` / `DECIMAL` | `SqlValue::Decimal(Decimal)`, at the column scale (`15.00`, not `15`); `SqlValue::NullDecimal` for NULL |
| `TIMESTAMPTZ` / `TIMESTAMP` | `SqlValue::DateTime(OffsetDateTime)` (UTC) |
| `DATE` | `SqlValue::Date(Date)` |
| `TIME` | `SqlValue::Time(Time)` |
| `JSON` / `JSONB` | `SqlValue::Json(Value)` |
| `BYTEA` | `SqlValue::Bytes(Vec<u8>)` |
| `NULL` | the typed null of the column (`NullDateTime`, `NullDate`, ...), `SqlValue::NullText` otherwise |

Decimal columns are decoded from the model's schema type, so SQLite, MySQL and MariaDB return the same `SqlValue::Decimal` as Postgres. MySQL / MariaDB return `DateTime`, `Date`, `Time` and `Json` for those column types too (MariaDB's JSON is text, so `Text`); a computed `DECIMAL` in `raw_values` is a `Decimal`. SQLite stores dates as text and returns `Text`.

A non-null value of a type Rango cannot read is an `Error::Decode` naming the column, never a null.

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
