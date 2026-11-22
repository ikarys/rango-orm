# Vector search (pgvector)

Store embeddings in a `vector` column and find the closest rows. PostgreSQL only: the server needs the pgvector extension.

## Setup

Use a PostgreSQL server with pgvector installed, for example the Docker image `pgvector/pgvector:pg16`. The first migration with a vector column runs `CREATE EXTENSION IF NOT EXISTS vector`; on a server without pgvector it fails there. If the extension is missing when a query runs, the error is `decode error: type named vector not found`.

## The field

Use `FieldVector<N>` for a vector column, where `N` is the dimension. `N` must be in `1..=16000`; the derive refuses other values. The column type is `vector(N)`. Use `Option<FieldVector<N>>` for a nullable column.

`FieldVector::<N>::new(Vec<f32>) -> Result<Self, FieldError>` returns an error for the wrong length, `"Expected N dimensions, got M"`, or for a NaN or infinite value. `?` converts a `FieldError` into `rango::Error::Invalid`. `TryFrom<Vec<f32>>` uses the same check. Serde deserialization uses the same check. `as_slice()`, `into_inner()`, and `Deref` to `[f32]` are available.

## Migrations and indexes

`rango makemigrations` on Postgres writes `CREATE EXTENSION IF NOT EXISTS vector;` at the top of a migration when it creates or alters a vector column. On SQLite or MySQL/MariaDB, `rango makemigrations` and `rango restore` into such a backend fail with:

```text
"table"."column" is a vector column: FieldVector needs the PostgreSQL backend (pgvector)
```

Declare the index on the field with the distance you search by:

```rust
#[field(index = "cosine")]   // or "l2", "inner_product"
embedding: FieldVector<384>,
```

`rango makemigrations` then writes an HNSW index with the matching operator class:

```sql
CREATE INDEX IF NOT EXISTS "doc_embedding_hnsw" ON "doc" USING hnsw ("embedding" vector_cosine_ops);
```

Changing the distance drops and recreates the index; removing the attribute drops it. Without an index, search is an exact scan. A bare `#[field(index)]` on a vector field, a distance on a non-vector field, or an unknown distance is a compile error.

## Searching

Query methods are on the `PgQueryExt` trait. Import it with `use rango::prelude::*;` or `use rango::PgQueryExt;`. Use `Distance` from `rango::Distance`; it is also in the prelude.

`Distance::L2` maps to `<->`, `Distance::Cosine` maps to `<=>`, and `Distance::InnerProduct` maps to `<#>`. pgvector returns the negative inner product, so smaller values are closer for all three distances.

`col` must be a vector column constant of the model (string column names, accepted by `filter` and `order_by`, are not accepted here), and `query` a `&FieldVector<N>` with the column's `N`. Anything else does not compile:

```text
error[E0277]: `Column<Doc, FieldText>` is not a vector column of `Doc`
```

`.nearest(col, &query, k, distance)` generates `ORDER BY col <op> $n LIMIT k`. The distance ordering comes first. A later `.order_by(...)` only breaks ties. It combines with any filter.

`.within(col, &query, max, distance)` generates `WHERE col <op> $n < max`. It is a radius filter and does not order results; like any `.all()`, it returns at most 1000 rows unless you set `.limit(n)` or `.unlimited()`.

On an `Option<FieldVector<N>>` column, rows where the vector is NULL are never returned by `nearest` or `within`.

`.all_with_distance()` returns `Vec<(M, f64)>`, each row with its distance to the query. It needs `.nearest()`, and does not combine with `select_related` or `prefetch_related` (both `Error::Invalid`). Postgres computes the distance once for the select list and the ordering, and still uses the HNSW index: it costs the same as `.all()` in the benchmark. `.aggregate()` with `.nearest()` is `Error::Invalid`. `.count()` ignores the ordering.

## When Postgres uses the HNSW index

These are pgvector's rules; check a query with `.explain_analyze()` (`.explain()` only returns the SQL).

- `nearest` alone uses the index when it asks for the distance the index was built with.
- `within` alone never uses it: pgvector indexes only serve `ORDER BY distance LIMIT k`. Add `nearest` to bound a radius search.
- `nearest` with a filter: pgvector filters the rows the index returns, so Postgres may prefer an exact scan, or return fewer than `k` rows when the filter is selective. For a selective filter, an index on the filter column lets Postgres scan only the matching rows exactly; pgvector 0.8 also has `SET hnsw.iterative_scan = relaxed_order`.

A large first load is faster without the index, which pgvector updates on every insert, so inserts slow down as the table grows. Load first, then add the `index` attribute and migrate.

## Example

Embeddings come from your own embedding model; `embed()` below stands for it and is out of scope.

```rust
use rango::prelude::*;

#[derive(Model, Debug, Clone)]
#[model(table = "doc")]
struct Doc {
    id: FieldUuid,
    title: FieldVarchar<1, 200>,
    #[field(index = "cosine")]
    embedding: FieldVector<384>,
}

async fn run(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let title = "Weekend trip to Lisbon";
    insert(pool, Doc {
        id: FieldUuid(uuid::Uuid::new_v4()),
        title: FieldVarchar::new(title.to_string())?,
        embedding: FieldVector::new(embed(title))?,
    })
    .await?;

    let query = FieldVector::new(embed("travel plans"))?;
    let closest: Vec<(Doc, f64)> = Doc::filter(pool)
        .nearest(Doc::EMBEDDING, &query, 5, Distance::Cosine)
        .all_with_distance()
        .await?;
    let similar = Doc::filter(pool)
        .within(Doc::EMBEDDING, &query, 0.25, Distance::Cosine)
        .all()
        .await?;
    Ok(())
}
```

## Performance

On 10 000 rows of 384 dimensions, a `nearest` query takes the same time through Rango as the
same SQL written by hand with sqlx, within 3%. Postgres uses the HNSW index for the query Rango
builds; the benchmark checks that with `EXPLAIN ANALYZE`. `.all_with_distance()` takes the same
time as `.all()` (1.300 ms against 1.299 ms). Numbers and method are in
[benchmarks/README.md](https://github.com/ikarys/rango-orm/tree/main/benchmarks#vector-search-pgvector-2026-11-22).

## Errors and safety

The column and the query dimension are checked by the compiler; a `FieldVector` only holds finite values. The operator is a fixed string. The column name comes from the model's column constant and is quoted. The query vector is bound as a parameter and is never formatted into the SQL.