/// Rango ORM benchmarks on PostgreSQL.
/// Same workloads as bench-sqlx-raw for direct comparison.
///
/// Run: DATABASE_URL=postgres://... cargo bench -p bench-rango
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use rango_orm_core::*;
use rango_orm_derive::Model;
use rango_orm_postgres::*;
use uuid::Uuid;

// ── Model ─────────────────────────────────────────────────────────────────────

#[derive(Model, Clone)]
#[model(table = "rango_bench_user")]
struct BenchUser {
    id: FieldUuid,
    email: FieldEmail,
    name: FieldVarchar<1, 100>,
    active: FieldBool,
    score: Option<FieldInt>,
    created_at: FieldDateTime,
}

// ── Setup ─────────────────────────────────────────────────────────────────────

async fn setup() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5433/rango_test".to_string());
    let pool = PgPool::connect(&url)
        .await
        .expect("Failed to connect — set DATABASE_URL");
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS "rango_bench_user" (
            "id"         UUID PRIMARY KEY,
            "email"      VARCHAR(254) NOT NULL,
            "name"       VARCHAR(100) NOT NULL,
            "active"     BOOLEAN NOT NULL,
            "score"      INTEGER,
            "created_at" TIMESTAMPTZ NOT NULL
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(r#"TRUNCATE "rango_bench_user""#)
        .execute(&pool)
        .await
        .unwrap();
    pool
}

fn make_user(i: usize) -> BenchUser {
    BenchUser {
        id: FieldUuid(Uuid::new_v4()),
        email: FieldEmail::new(format!("u{}@bench.com", i)).unwrap(),
        name: FieldVarchar::new("Bench User").unwrap(),
        active: FieldBool(true),
        score: Some(FieldInt(i as i32)),
        created_at: FieldDateTime::now(),
    }
}

// ── insert_one ────────────────────────────────────────────────────────────────

fn bench_insert_one(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(setup());

    c.bench_function("rango/insert_one", |b| {
        b.iter_batched(
            || make_user(0),
            |user| rt.block_on(async { insert(&pool, user).await.unwrap() as BenchUser }),
            BatchSize::SmallInput,
        )
    });
}

// ── bulk_insert ───────────────────────────────────────────────────────────────

fn bench_bulk_insert(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(setup());
    let mut group = c.benchmark_group("rango/bulk_insert");

    for size in [10usize, 100, 1000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_batched(
                || (0..size).map(make_user).collect::<Vec<_>>(),
                |users| rt.block_on(async { bulk_create(&pool, &users).await.unwrap() }),
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

// ── select ────────────────────────────────────────────────────────────────────

fn bench_select(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (pool, first_id) = rt.block_on(async {
        let pool = setup().await;
        let users: Vec<BenchUser> = (0..1000).map(make_user).collect();
        bulk_create(&pool, &users).await.unwrap();
        let pk = users[0].id.to_sql_value();
        (pool, pk)
    });

    c.bench_function("rango/select_pk", |b| {
        b.iter(|| rt.block_on(async { get::<_, BenchUser>(&pool, &first_id).await.unwrap() }))
    });

    c.bench_function("rango/select_filter_100", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchUser::filter(&pool)
                    .eq("active", true)
                    .limit(100)
                    .all()
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("rango/select_all_1000", |b| {
        b.iter(|| rt.block_on(async { BenchUser::filter(&pool).unlimited().all().await.unwrap() }))
    });

    c.bench_function("rango/count", |b| {
        b.iter(|| rt.block_on(async { BenchUser::filter(&pool).count().await.unwrap() }))
    });
}

// ── update_one ────────────────────────────────────────────────────────────────

fn bench_update(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (pool, user) = rt.block_on(async {
        let pool = setup().await;
        let u: BenchUser = insert(&pool, make_user(0)).await.unwrap();
        (pool, u)
    });

    c.bench_function("rango/update_one", |b| {
        b.iter(|| {
            let mut u = user.clone();
            u.name = FieldVarchar::new("Updated").unwrap();
            rt.block_on(async { update(&pool, u).await.unwrap() })
        })
    });
}

// ── transaction ───────────────────────────────────────────────────────────────

fn bench_transaction(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(setup());

    c.bench_function("rango/transaction_10_inserts", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut tx = pool.begin().await.unwrap();
                for i in 0..10usize {
                    insert(&mut *tx, make_user(i)).await.unwrap();
                }
                tx.commit().await.unwrap()
            })
        })
    });
}

criterion_group!(
    benches,
    bench_insert_one,
    bench_bulk_insert,
    bench_select,
    bench_update,
    bench_transaction,
    bench_deserialize_only,
    bench_vector,
);
criterion_main!(benches);

fn bench_deserialize_only(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(async {
        let pool = setup().await;
        let users: Vec<BenchUser> = (0..1000).map(make_user).collect();
        bulk_create(&pool, &users).await.unwrap();
        pool
    });

    // Fetch raw rows without ORM deserialization
    c.bench_function("rango/fetch_raw_rows_1000", |b| {
        b.iter(|| {
            rt.block_on(async {
                sqlx::query(r#"SELECT * FROM "rango_bench_user" LIMIT 1000"#)
                    .fetch_all(&pool)
                    .await
                    .unwrap()
            })
        })
    });

    // Full ORM deserialization
    c.bench_function("rango/fetch_orm_1000", |b| {
        b.iter(|| rt.block_on(async { BenchUser::filter(&pool).unlimited().all().await.unwrap() }))
    });
}
// ── pgvector ──────────────────────────────────────────────────────────────────
// Same data as bench-sqlx-raw: 10k deterministic unit vectors of 384 dimensions (the size of
// common sentence-embedding models), searched by cosine distance.

const DOCS: usize = 10_000;
const DIM: usize = 384;

#[derive(Model, Clone)]
#[model(table = "bench_doc")]
struct BenchDoc {
    id: FieldBigInt,
    embedding: FieldVector<384>,
}

#[derive(Model, Clone)]
#[model(table = "bench_doc_exact")]
struct BenchDocExact {
    id: FieldBigInt,
    embedding: FieldVector<384>,
}

#[derive(Model, Clone)]
#[model(table = "bench_doc_ins")]
struct BenchDocIns {
    id: FieldUuid,
    embedding: FieldVector<384>,
}

/// xorshift64, so both suites build the same vectors without a rand dependency.
fn unit_vector(seed: u64) -> Vec<f32> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let v: Vec<f32> = (0..DIM)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 40) as f32 / (1u64 << 24) as f32 - 0.5
        })
        .collect();
    let norm = v.iter().map(|a| a * a).sum::<f32>().sqrt();
    v.into_iter().map(|a| a / norm).collect()
}

async fn vector_setup() -> PgPool {
    let pool = setup().await;
    sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
        .execute(&pool)
        .await
        .unwrap();
    for t in ["bench_doc", "bench_doc_exact"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"CREATE TABLE IF NOT EXISTS "{t}" ("id" BIGINT PRIMARY KEY, "embedding" vector(384) NOT NULL)"#
        )))
        .execute(&pool).await.unwrap();
    }
    sqlx::query(r#"CREATE TABLE IF NOT EXISTS "bench_doc_ins" ("id" UUID PRIMARY KEY, "embedding" vector(384) NOT NULL)"#)
        .execute(&pool).await.unwrap();
    let have = BenchDoc::filter(&pool).count().await.unwrap();
    let exact = BenchDocExact::filter(&pool).count().await.unwrap();
    if have != DOCS as i64 || exact != DOCS as i64 {
        sqlx::query(r#"TRUNCATE "bench_doc", "bench_doc_exact""#)
            .execute(&pool)
            .await
            .unwrap();
        for chunk in (0..DOCS).collect::<Vec<_>>().chunks(1000) {
            let docs: Vec<BenchDoc> = chunk
                .iter()
                .map(|&i| BenchDoc {
                    id: FieldBigInt(i as i64),
                    embedding: FieldVector::new(unit_vector(i as u64)).unwrap(),
                })
                .collect();
            bulk_create(&pool, &docs).await.unwrap();
        }
        sqlx::query(r#"INSERT INTO "bench_doc_exact" SELECT * FROM "bench_doc""#)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(r#"CREATE INDEX IF NOT EXISTS "bench_doc_hnsw" ON "bench_doc" USING hnsw ("embedding" vector_cosine_ops)"#)
            .execute(&pool).await.unwrap();
        sqlx::query(r#"ANALYZE "bench_doc", "bench_doc_exact""#)
            .execute(&pool)
            .await
            .unwrap();
    }
    pool
}

fn bench_vector(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(vector_setup());
    let query = FieldVector::<DIM>::new(unit_vector(DOCS as u64 + 1)).unwrap();

    // The SQL rango generates (with the vector as a bound parameter) must reach the HNSW index.
    let plan = rt.block_on(async {
        BenchDoc::filter(&pool)
            .nearest(BenchDoc::EMBEDDING, &query, 10, Distance::Cosine)
            .explain_analyze()
            .await
            .unwrap()
    });
    assert!(
        plan.contains("bench_doc_hnsw"),
        "HNSW index not used:\n{plan}"
    );

    let mut group = c.benchmark_group("rango/vector_nearest_10");
    group.bench_function("hnsw", |b| {
        b.iter(|| {
            rt.block_on(async {
                let r = BenchDoc::filter(&pool)
                    .nearest(BenchDoc::EMBEDDING, &query, 10, Distance::Cosine)
                    .all()
                    .await
                    .unwrap();
                assert_eq!(r.len(), 10);
                r
            })
        })
    });
    group.bench_function("hnsw_with_distance", |b| {
        b.iter(|| {
            rt.block_on(async {
                let r = BenchDoc::filter(&pool)
                    .nearest(BenchDoc::EMBEDDING, &query, 10, Distance::Cosine)
                    .all_with_distance()
                    .await
                    .unwrap();
                assert_eq!(r.len(), 10);
                r
            })
        })
    });
    group.bench_function("exact", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchDocExact::filter(&pool)
                    .nearest(BenchDocExact::EMBEDDING, &query, 10, Distance::Cosine)
                    .all()
                    .await
                    .unwrap()
            })
        })
    });
    group.finish();

    c.bench_function("rango/vector_bulk_insert/100", |b| {
        b.iter_batched(
            || {
                (0..100u64)
                    .map(|i| BenchDocIns {
                        id: FieldUuid(Uuid::new_v4()),
                        embedding: FieldVector::new(unit_vector(i)).unwrap(),
                    })
                    .collect::<Vec<_>>()
            },
            |docs| rt.block_on(async { bulk_create(&pool, &docs).await.unwrap() }),
            BatchSize::SmallInput,
        )
    });
}
