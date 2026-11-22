# Vector search (pgvector)

Store embeddings in a `vector` column and find the closest rows. PostgreSQL only: the server needs the pgvector extension.

## Setup

Use a PostgreSQL server with pgvector installed, for example the Docker image `pgvector/pgvector:pg16`.

## The field

Use `FieldVector<N>` for a vector column, where `N` is the dimension. `N` must be in `1..=16000`; the derive refuses other values. The column type is `vector(N)`. Use `Option<FieldVector<N>>` for a nullable column.

`FieldVector::<N>::new(Vec<f32>) -> Result<Self, String>` returns an error for the wrong length, `"Expected N dimensions, got M"`, or for a NaN or infinite value. `TryFrom<Vec<f32>>` uses the same check. Serde deserialization uses the same check. `as_slice()`, `into_inner()`, and `Deref` to `[f32]` are available.

## Migrations and indexes

`rango makemigrations` on Postgres writes `CREATE EXTENSION IF NOT EXISTS vector;` at the top of a migration when it creates or alters a vector column. On SQLite or MySQL/MariaDB, `rango makemigrations` and `rango restore` into such a backend fail with:

```text
"table"."column" is a vector column: FieldVector needs the PostgreSQL backend (pgvector)
```

rango does not generate HNSW or IVFFlat indexes. Add an index by hand in the migration file:

```sql
CREATE INDEX ON "doc" USING hnsw ("embedding" vector_cosine_ops);
```

Use `vector_l2_ops` for L2 distance and `vector_ip_ops` for inner product. Without an index, search is an exact scan.

## Searching

Query methods are on the `PgQueryExt` trait. Import it with `use rango::prelude::*;` or `use rango::PgQueryExt;`. Use `Distance` from `rango::Distance`; it is also in the prelude.

`Distance::L2` maps to `<->`, `Distance::Cosine` maps to `<=>`, and `Distance::InnerProduct` maps to `<#>`. pgvector returns the negative inner product, so smaller values are closer for all three distances.

`.nearest(col, &query, k, distance)` generates `ORDER BY col <op> $n LIMIT k`. The distance ordering comes first. A later `.order_by(...)` only breaks ties. It combines with any filter.

`.within(col, &query, max, distance)` generates `WHERE col <op> $n < max`. It is a radius filter and does not order results.

The distance value itself is not returned. `.aggregate()` with `.nearest()` is `Error::Invalid`. `.count()` ignores the ordering.

## Example

Embeddings come from your own embedding model; `embed()` below stands for it and is out of scope.

```rust
use rango::prelude::*;

#[derive(Model, Debug, Clone)]
#[model(table = "doc")]
struct Doc {
    id: FieldUuid,
    title: FieldVarchar<1, 200>,
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

    let query: Vec<f32> = embed("travel plans");
    let closest = Doc::filter(pool)
        .nearest(Doc::EMBEDDING, &query, 5, Distance::Cosine)
        .all()
        .await?;
    let similar = Doc::filter(pool)
        .within(Doc::EMBEDDING, &query, 0.25, Distance::Cosine)
        .all()
        .await?;
    Ok(())
}
```

## Errors and safety

The column must be a vector column of the model. The query vector must have the column's dimension and finite values. Otherwise rango returns `Error::Invalid` before any SQL runs. The operator is a fixed string. The column name is checked against the model and quoted. The query vector is bound as a parameter and is never formatted into the SQL.