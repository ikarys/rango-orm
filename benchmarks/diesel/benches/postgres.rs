use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use diesel::pg::Pg;
use diesel::prelude::*;
use diesel_async::pooled_connection::{AsyncDieselConnectionManager, deadpool::Pool};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};

type PgPool = Pool<AsyncPgConnection>;

diesel::table! {
    diesel_bench_user (id) {
        id -> Uuid,
        email -> Varchar,
        name -> Varchar,
        active -> Bool,
        score -> Nullable<Integer>,
        created_at -> Timestamptz,
    }
}

#[derive(Queryable, Selectable, Insertable, AsChangeset, Clone, Debug)]
#[diesel(table_name = diesel_bench_user)]
#[diesel(check_for_backend(Pg))]
struct BenchUser {
    id: uuid::Uuid,
    email: String,
    name: String,
    active: bool,
    score: Option<i32>,
    created_at: chrono::DateTime<chrono::Utc>,
}

async fn setup() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5433/rango_test".to_string());

    let manager = AsyncDieselConnectionManager::new(url);
    let pool: PgPool = Pool::builder(manager)
        .max_size(10)
        .build()
        .expect("Failed to connect — set DATABASE_URL");

    let mut conn = pool.get().await.expect("Failed to get connection");

    diesel::sql_query(
        r#"CREATE TABLE IF NOT EXISTS "diesel_bench_user" (
            "id"         UUID PRIMARY KEY,
            "email"      VARCHAR(254) NOT NULL,
            "name"       VARCHAR(100) NOT NULL,
            "active"     BOOLEAN NOT NULL,
            "score"      INTEGER,
            "created_at" TIMESTAMPTZ NOT NULL
        )"#,
    )
    .execute(&mut conn)
    .await
    .unwrap();

    diesel::sql_query(r#"TRUNCATE "diesel_bench_user""#)
        .execute(&mut conn)
        .await
        .unwrap();

    pool
}

fn make_user(i: usize) -> BenchUser {
    BenchUser {
        id: uuid::Uuid::new_v4(),
        email: format!("u{}@bench.com", i),
        name: "Bench User".to_string(),
        active: true,
        score: Some(i as i32),
        created_at: chrono::Utc::now(),
    }
}

fn bench_insert_one(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(setup());

    c.bench_function("diesel_async/insert_one", |b| {
        b.iter_batched(
            || make_user(0),
            |user| {
                rt.block_on(async {
                    let mut conn = pool.get().await.unwrap();
                    diesel::insert_into(diesel_bench_user::table)
                        .values(&user)
                        .get_result::<BenchUser>(&mut conn)
                        .await
                        .unwrap()
                })
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_bulk_insert(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(setup());
    let mut group = c.benchmark_group("diesel_async/bulk_insert");

    for size in [10usize, 100, 1000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_batched(
                || (0..size).map(make_user).collect::<Vec<_>>(),
                |users| {
                    rt.block_on(async {
                        let mut conn = pool.get().await.unwrap();
                        diesel::insert_into(diesel_bench_user::table)
                            .values(&users)
                            .execute(&mut conn)
                            .await
                            .unwrap();
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
    let (pool, first_id) = rt.block_on(async {
        let pool = setup().await;
        let users: Vec<BenchUser> = (0..1000).map(make_user).collect();

        let mut conn = pool.get().await.unwrap();
        diesel::insert_into(diesel_bench_user::table)
            .values(&users)
            .execute(&mut conn)
            .await
            .unwrap();

        let pk = users[0].id;
        (pool, pk)
    });

    c.bench_function("diesel_async/select_pk", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut conn = pool.get().await.unwrap();
                diesel_bench_user::table
                    .find(first_id)
                    .first::<BenchUser>(&mut conn)
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("diesel_async/select_filter_100", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut conn = pool.get().await.unwrap();
                diesel_bench_user::table
                    .filter(diesel_bench_user::active.eq(true))
                    .limit(100)
                    .load::<BenchUser>(&mut conn)
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("diesel_async/select_all_1000", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut conn = pool.get().await.unwrap();
                diesel_bench_user::table.load::<BenchUser>(&mut conn).await.unwrap()
            })
        })
    });

    c.bench_function("diesel_async/count", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut conn = pool.get().await.unwrap();
                diesel_bench_user::table
                    .count()
                    .get_result::<i64>(&mut conn)
                    .await
                    .unwrap()
            })
        })
    });
}

fn bench_update(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (pool, user) = rt.block_on(async {
        let pool = setup().await;
        let mut conn = pool.get().await.unwrap();
        let u = diesel::insert_into(diesel_bench_user::table)
            .values(make_user(0))
            .get_result::<BenchUser>(&mut conn)
            .await
            .unwrap();
        (pool, u)
    });

    c.bench_function("diesel_async/update_one", |b| {
        b.iter(|| {
            let mut u = user.clone();
            u.name = "Updated".to_string();
            rt.block_on(async {
                let mut conn = pool.get().await.unwrap();
                diesel::update(diesel_bench_user::table.find(u.id))
                    .set(&u)
                    .get_result::<BenchUser>(&mut conn)
                    .await
                    .unwrap()
            })
        })
    });
}

fn bench_transaction(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(setup());

    c.bench_function("diesel_async/transaction_10_inserts", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut conn = pool.get().await.unwrap();
                conn.transaction::<_, diesel::result::Error, _>(async |c| {
                    for i in 0..10usize {
                        diesel::insert_into(diesel_bench_user::table)
                            .values(make_user(i))
                            .execute(c)
                            .await?;
                    }
                    Ok(())
                })
                .await
                .unwrap();
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
);
criterion_main!(benches);
