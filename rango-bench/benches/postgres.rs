/// PostgreSQL benchmarks — requires DATABASE_URL env var.
///
/// Run with:
///   DATABASE_URL=postgres://... cargo bench -p rango-bench --bench postgres
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use rango_core::*;
use rango_derive::Model;
use rango_postgres::*;
use uuid::Uuid;

// ── Test model ────────────────────────────────────────────────────────────────

#[derive(Model, Clone)]
#[model(table = "bench_pg_user")]
struct BenchPgUser {
    id: FieldUuid,
    email: FieldEmail,
    name: FieldVarchar<1, 100>,
    active: FieldBool,
    score: Option<FieldInt>,
}

// ── Setup ─────────────────────────────────────────────────────────────────────

async fn setup() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/rango_bench".to_string());
    let pool = PgPool::connect(&url)
        .await
        .expect("Failed to connect to Postgres — set DATABASE_URL");
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS "bench_pg_user" (
            "id"     UUID PRIMARY KEY,
            "email"  VARCHAR(254) NOT NULL,
            "name"   VARCHAR(100) NOT NULL,
            "active" BOOLEAN NOT NULL,
            "score"  INTEGER
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(r#"TRUNCATE "bench_pg_user""#)
        .execute(&pool)
        .await
        .unwrap();
    pool
}

fn make_user(i: usize) -> BenchPgUser {
    BenchPgUser {
        id: FieldUuid(Uuid::new_v4()),
        email: FieldEmail(format!("user{}@bench.com", i)),
        name: FieldVarchar(format!("User {}", i)),
        active: FieldBool(true),
        score: Some(FieldInt(i as i32)),
    }
}

// ── Benchmarks ────────────────────────────────────────────────────────────────

fn bench_insert(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(setup());

    c.bench_function("postgres/insert_one", |b| {
        b.iter_batched(
            || make_user(0),
            |user| rt.block_on(async { insert(&pool, user).await.unwrap() }),
            BatchSize::SmallInput,
        )
    });
}

fn bench_bulk_create(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(setup());
    let mut group = c.benchmark_group("postgres/bulk_create");

    for size in [10, 100, 1000] {
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

fn bench_filter(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(async {
        let pool = setup().await;
        let users: Vec<BenchPgUser> = (0..1000).map(make_user).collect();
        bulk_create(&pool, &users).await.unwrap();
        pool
    });

    c.bench_function("postgres/filter_all_1000", |b| {
        b.iter(|| {
            rt.block_on(async { BenchPgUser::filter(&pool).unlimited().all().await.unwrap() })
        })
    });

    c.bench_function("postgres/filter_eq", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchPgUser::filter(&pool)
                    .eq("active", true)
                    .limit(100)
                    .all()
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("postgres/count", |b| {
        b.iter(|| rt.block_on(async { BenchPgUser::filter(&pool).count().await.unwrap() }))
    });
}

fn bench_aggregations(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(async {
        let pool = setup().await;
        let users: Vec<BenchPgUser> = (0..1000).map(make_user).collect();
        bulk_create(&pool, &users).await.unwrap();
        pool
    });

    c.bench_function("postgres/sum", |b| {
        b.iter(|| rt.block_on(async { BenchPgUser::filter(&pool).sum("score").await.unwrap() }))
    });

    c.bench_function("postgres/avg", |b| {
        b.iter(|| rt.block_on(async { BenchPgUser::filter(&pool).avg("score").await.unwrap() }))
    });
}

criterion_group!(
    benches,
    bench_insert,
    bench_bulk_create,
    bench_filter,
    bench_aggregations
);
criterion_main!(benches);
