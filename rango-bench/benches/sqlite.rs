/// SQLite benchmarks — in-memory, no external deps needed.
///
/// Run with:
///   cargo bench -p rango-bench --bench sqlite
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use rango_core::*;
use rango_derive::Model;
use rango_sqlite::*;
use uuid::Uuid;

// ── Test model ────────────────────────────────────────────────────────────────

#[derive(Model, Clone)]
#[model(table = "bench_user")]
struct BenchUser {
    id: FieldUuid,
    email: FieldEmail,
    name: FieldVarchar<1, 100>,
    active: FieldBool,
    score: Option<FieldInt>,
}

// ── Setup ─────────────────────────────────────────────────────────────────────

async fn setup() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query(
        r#"CREATE TABLE "bench_user" (
            "id"     TEXT PRIMARY KEY,
            "email"  TEXT NOT NULL,
            "name"   TEXT NOT NULL,
            "active" INTEGER NOT NULL,
            "score"  INTEGER
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn make_user(i: usize) -> BenchUser {
    BenchUser {
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

    c.bench_function("sqlite/insert_one", |b| {
        b.iter_batched(
            || make_user(0),
            |user| {
                rt.block_on(async {
                    insert(&pool, user).await.unwrap();
                })
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_bulk_create(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(setup());
    let mut group = c.benchmark_group("sqlite/bulk_create");

    for size in [10, 100, 1000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_batched(
                || (0..size).map(make_user).collect::<Vec<_>>(),
                |users| {
                    rt.block_on(async {
                        bulk_create(&pool, &users).await.unwrap();
                    })
                },
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
        let users: Vec<BenchUser> = (0..1000).map(make_user).collect();
        bulk_create(&pool, &users).await.unwrap();
        pool
    });

    c.bench_function("sqlite/filter_all_1000", |b| {
        b.iter(|| rt.block_on(async { BenchUser::filter(&pool).unlimited().all().await.unwrap() }))
    });

    c.bench_function("sqlite/filter_eq", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchUser::filter(&pool)
                    .eq("active", 1i32)
                    .limit(100)
                    .all()
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("sqlite/count", |b| {
        b.iter(|| rt.block_on(async { BenchUser::filter(&pool).count().await.unwrap() }))
    });

    c.bench_function("sqlite/values_projection", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchUser::filter(&pool)
                    .limit(100)
                    .values(&["id", "email"])
                    .await
                    .unwrap()
            })
        })
    });
}

fn bench_aggregations(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(async {
        let pool = setup().await;
        let users: Vec<BenchUser> = (0..1000).map(make_user).collect();
        bulk_create(&pool, &users).await.unwrap();
        pool
    });

    c.bench_function("sqlite/sum", |b| {
        b.iter(|| rt.block_on(async { BenchUser::filter(&pool).sum("score").await.unwrap() }))
    });

    c.bench_function("sqlite/avg", |b| {
        b.iter(|| rt.block_on(async { BenchUser::filter(&pool).avg("score").await.unwrap() }))
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
