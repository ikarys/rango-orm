use butane::db::{connect_async, BackendConnectionAsync, ConnectionAsync, ConnectionSpec};
use butane::prelude_async::*;
use butane::{model, query};
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use uuid::Uuid;

#[model]
#[table = "butane_bench_user"]
#[derive(Clone, Debug)]
struct BenchUser {
    #[pk]
    id: Uuid,
    email: String,
    name: String,
    active: bool,
    score: Option<i32>,
    created_at: chrono::NaiveDateTime,
}

async fn setup() -> ConnectionAsync {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5433/rango_test".to_string());

    let conn: ConnectionAsync = connect_async(&ConnectionSpec::new("pg", url))
        .await
        .unwrap();

    conn.execute(
        r#"CREATE TABLE IF NOT EXISTS butane_bench_user (
            id BYTEA PRIMARY KEY,
            email TEXT NOT NULL,
            name TEXT NOT NULL,
            active BOOLEAN NOT NULL,
            score INTEGER,
            created_at TIMESTAMP NOT NULL
        )"#,
    )
    .await
    .unwrap();

    conn.execute("TRUNCATE butane_bench_user").await.unwrap();

    conn
}

fn make_user(i: usize) -> BenchUser {
    BenchUser {
        id: Uuid::new_v4(),
        email: format!("u{}@bench.com", i),
        name: "Bench User".to_string(),
        active: true,
        score: Some(i as i32),
        created_at: chrono::Utc::now().naive_utc(),
    }
}

fn bench_insert_one(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let conn = rt.block_on(setup());

    c.bench_function("butane/insert_one", |b| {
        b.iter_batched(
            || make_user(0),
            |mut u| rt.block_on(async { u.save(&conn).await.unwrap() }),
            BatchSize::SmallInput,
        )
    });
}

fn bench_bulk_insert(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut conn = rt.block_on(setup());
    let mut group = c.benchmark_group("butane/bulk_insert");

    for size in [10usize, 100, 1000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_batched(
                || (0..size).map(make_user).collect::<Vec<_>>(),
                |users| {
                    rt.block_on(async {
                        // Butane has no multi-row insert.
                        let tx = conn.transaction().await.unwrap();
                        for mut u in users {
                            u.save(&tx).await.unwrap();
                        }
                        tx.commit().await.unwrap();
                    })
                },
                BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

fn bench_select(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (conn, first_id) = rt.block_on(async {
        let mut conn = setup().await;
        let users: Vec<BenchUser> = (0..1000).map(make_user).collect();
        let first_id = users[0].id;
        let tx = conn.transaction().await.unwrap();
        for mut u in users {
            u.save(&tx).await.unwrap();
        }
        tx.commit().await.unwrap();
        (conn, first_id)
    });

    c.bench_function("butane/select_pk", |b| {
        b.iter(|| rt.block_on(async { BenchUser::get(&conn, first_id).await.unwrap() }))
    });

    c.bench_function("butane/select_filter_100", |b| {
        b.iter(|| {
            rt.block_on(async {
                query!(BenchUser, active == true)
                    .limit(100)
                    .load(&conn)
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("butane/select_all_1000", |b| {
        b.iter(|| rt.block_on(async { BenchUser::query().load(&conn).await.unwrap() }))
    });
}

fn bench_update(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (conn, user) = rt.block_on(async {
        let conn = setup().await;
        let mut user = make_user(0);
        user.save(&conn).await.unwrap();
        (conn, user)
    });

    c.bench_function("butane/update_one", |b| {
        b.iter(|| {
            let mut user = user.clone();
            user.name = "Updated".to_string();
            rt.block_on(async { user.save(&conn).await.unwrap() })
        })
    });
}

fn bench_transaction(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut conn = rt.block_on(setup());

    c.bench_function("butane/transaction_10_inserts", |b| {
        b.iter(|| {
            rt.block_on(async {
                let tx = conn.transaction().await.unwrap();
                for i in 0..10usize {
                    let mut u = make_user(i);
                    u.save(&tx).await.unwrap();
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
    bench_transaction
);
criterion_main!(benches);
