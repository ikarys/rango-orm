# Aggregations

## sum / avg

Return `Option<f64>` — `None` if the table is empty or all values are NULL.

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
