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

## Grouping: `group_by`, `annotate_*`, `having`

One query with `GROUP BY` and several aggregates, the Django `annotate()` + `aggregate()` pair in one call:

```rust
use rango::Cmp;

let rows = Order::filter(&pool)
    .eq(Order::PAID, true)
    .group_by(Order::COUNTRY)              // repeatable
    .annotate_count("n")                   // COUNT(*) AS "n"
    .annotate_sum("total", Order::AMOUNT)  // SUM("amount") AS "total"
    .having("total", Cmp::Gt, 100.0)       // HAVING SUM("amount") > $2
    .order_by("-total")                    // an annotation alias works here
    .aggregate()
    .await?;                               // Vec<HashMap<String, SqlValue>>

for row in &rows {
    println!("{:?}: {:?} orders, {:?} total", row["country"], row["n"], row["total"]);
}
```

```sql
SELECT "country", COUNT(*) AS "n", SUM("amount")::float8 AS "total"
FROM "order" WHERE "paid" = $1
GROUP BY "country" HAVING SUM("amount")::float8 > $2
ORDER BY "total" DESC LIMIT 1000
```

Without `group_by`, `aggregate()` returns one row holding every aggregate:

```rust
let row = Order::filter(&pool)
    .annotate_count("n")
    .annotate_max("latest", Order::CREATED_AT)
    .aggregate()
    .await?;
```

Rules:

- Each map holds the group columns and the aliases. `COUNT` is `SqlValue::BigInt`, `SUM` / `AVG` are `SqlValue::Double`, `MIN` / `MAX` keep the column type (a `FieldDecimal` column stays exact). A `FieldBool` group column comes back as `Bool` on Postgres and as an integer on SQLite and MySQL.
- `having()` takes an alias declared by an `annotate_*` call; the SQL repeats the aggregate expression (Postgres cannot use an output alias in `HAVING`). Several calls are joined with `AND`, values are bound.
- `where` conditions (`eq`, `like`, ...) filter rows before grouping, `having` filters groups after.
- `order_by`, `limit`, `offset` and the default cap of 1000 rows apply to the groups. The model's default `ordering` is not applied (it would name columns outside the `GROUP BY`): order groups with an explicit `order_by`. `select_related` / `prefetch_related` are `Error::Invalid`.
- An unknown column, alias or shape is `Error::Invalid` before any SQL runs.
- `group_by` takes columns only, not expressions: grouping by month (`date_trunc('month', created_at)`) needs [raw SQL](raw-sql.md). `having` compares an aggregate to a bound constant; it cannot reference a column of another table ("bookings above 90% of the event's capacity"): use raw SQL, or two queries compared in Rust.
