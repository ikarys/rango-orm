/// Rango ORM benchmarks on PostgreSQL.
/// Same workloads as bench-sqlx-raw for direct comparison.
///
/// Run: DATABASE_URL=postgres://... cargo bench -p bench-rango
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use rango_core::*;
use rango_derive::Model;
use rango_postgres::*;
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
    let pool = PgPool::connect(&url).await.expect("Failed to connect — set DATABASE_URL");
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
    .execute(&pool).await.unwrap();
    sqlx::query(r#"TRUNCATE "rango_bench_user""#)
        .execute(&pool).await.unwrap();
    pool
}

fn make_user(i: usize) -> BenchUser {
    BenchUser {
        id: FieldUuid(Uuid::new_v4()),
        email: FieldEmail(format!("u{}@bench.com", i)),
        name: FieldVarchar("Bench User".into()),
        active: FieldBool(true),
        score: Some(FieldInt(i as i32)),
        created_at: FieldDateTime(chrono::Utc::now()),
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
        b.iter(|| {
            rt.block_on(async { get::<_, BenchUser>(&pool, &first_id).await.unwrap() })
        })
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
        b.iter(|| {
            rt.block_on(async {
                BenchUser::filter(&pool).unlimited().all().await.unwrap()
            })
        })
    });

    c.bench_function("rango/count", |b| {
        b.iter(|| {
            rt.block_on(async { BenchUser::filter(&pool).count().await.unwrap() })
        })
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
            u.name = FieldVarchar("Updated".into());
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
        b.iter(|| {
            rt.block_on(async {
                BenchUser::filter(&pool).unlimited().all().await.unwrap()
            })
        })
    });
}
