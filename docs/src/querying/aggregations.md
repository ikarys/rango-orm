# Aggregations

## sum / avg

Return `Result<Option<f64>>`: `None` if the table is empty or all values are NULL. The database casts the result to a float (`float8` / `DOUBLE` / `REAL`), so a `FieldDecimal` column loses its exactness here.

```rust
let total = Order::filter(&pool).sum("amount").await?;
let average = Order::filter(&pool).avg("score").await?;

// With filters
let avg_active = User::filter(&pool)
    .eq("active", true)
    .avg("score")
    .await?;
```

## min / max

Generic over the return type — works for numbers, strings, dates.

### Exact decimals

`FieldDecimal` columns: `min::<Decimal>` and `max::<Decimal>` compile on PostgreSQL and MySQL / MariaDB only. On SQLite `Decimal` cannot be decoded, so they do not compile. `sum` and `avg` return `f64` on every backend, never an exact decimal.

The portable way to get an exact extreme is to read the row:

```rust
let priciest = Order::filter(&pool).order_by("-fee").limit(1).one().await?; // Option<Order>
let max_fee: Option<Decimal> = priciest.map(|o| o.fee.0);
```

For an exact total, read the rows (or `values()`) and add the `Decimal`s in Rust.

```rust
// Numeric
let lowest: Option<f64>  = Order::filter(&pool).min::<f64>("price").await?;
let highest: Option<f64> = Order::filter(&pool).max::<f64>("price").await?;

// Dates
let oldest: Option<chrono::NaiveDate> = Event::filter(&pool).min::<chrono::NaiveDate>("date").await?;
let latest: Option<chrono::DateTime<chrono::Utc>> = Post::filter(&pool)
    .eq("published", true)
    .max::<chrono::DateTime<chrono::Utc>>("created_at")
    .await?;

// Strings
let first: Option<String> = User::filter(&pool).min::<String>("name").await?;
```

## count

```rust
let total = User::filter(&pool).count().await?;

let active_count = User::filter(&pool)
    .eq("active", true)
    .count()
    .await?;
```

## exists

```rust
let has_admins = User::filter(&pool)
    .eq("role", "admin")
    .exists()
    .await?;

if has_admins {
    println!("At least one admin exists");
}
```
