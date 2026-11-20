/// PostgreSQL benchmarks — requires DATABASE_URL env var.
///
/// The `update_where`, `group_by` and `m2m_add` groups compare a Rango API
/// with the hand-written loop it replaces.
///
/// Run with:
///   DATABASE_URL=postgres://... cargo bench -p rango-bench --bench postgres
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use rango_orm_core::*;
use rango_orm_derive::Model;
use rango_orm_postgres::*;
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

#[derive(Model, Clone)]
#[model(table = "bench_pg_blog")]
struct BenchPgBlog {
    id: FieldUuid,
    title: FieldVarchar<1, 100>,
    tags: ManyToMany<BenchPgTag>,
}

#[derive(Model, Clone)]
#[model(table = "bench_pg_tag")]
struct BenchPgTag {
    id: FieldUuid,
    name: FieldVarchar<1, 100>,
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
        email: FieldEmail::new(format!("user{}@bench.com", i)).unwrap(),
        name: FieldVarchar::new(format!("User {}", i)).unwrap(),
        active: FieldBool(true),
        score: Some(FieldInt(i as i32)),
    }
}

async fn setup_m2m(pool: &PgPool) -> Vec<BenchPgTag> {
    sqlx::query(
        r#"DROP TABLE IF EXISTS "bench_pg_blog_bench_pg_tag", "bench_pg_blog", "bench_pg_tag""#,
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"CREATE TABLE "bench_pg_blog" (
            "id"    UUID PRIMARY KEY,
            "title" VARCHAR(100) NOT NULL
        )"#,
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"CREATE TABLE "bench_pg_tag" (
            "id"   UUID PRIMARY KEY,
            "name" VARCHAR(100) NOT NULL
        )"#,
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"CREATE TABLE "bench_pg_blog_bench_pg_tag" (
            "bench_pg_blog_id" UUID NOT NULL,
            "bench_pg_tag_id"  UUID NOT NULL,
            PRIMARY KEY ("bench_pg_blog_id", "bench_pg_tag_id")
        )"#,
    )
    .execute(pool)
    .await
    .unwrap();

    let tags: Vec<BenchPgTag> = (0..100)
        .map(|i| BenchPgTag {
            id: FieldUuid(Uuid::new_v4()),
            name: FieldVarchar::new(format!("tag{}", i)).unwrap(),
        })
        .collect();
    bulk_create(pool, &tags).await.unwrap();
    tags
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

fn bench_queryset_update(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(async {
        let pool = setup().await;
        let users: Vec<BenchPgUser> = (0..1000).map(make_user).collect();
        bulk_create(&pool, &users).await.unwrap();
        pool
    });

    let mut group = c.benchmark_group("postgres/update_where_1000");
    group.sample_size(20);

    group.bench_function("rango", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchPgUser::filter(&pool)
                    .eq(BenchPgUser::ACTIVE, true)
                    .set(BenchPgUser::SCORE, FieldInt(7))
                    .update()
                    .await
                    .unwrap()
            })
        })
    });

    group.bench_function("naive", |b| {
        b.iter(|| {
            rt.block_on(async {
                let users = BenchPgUser::filter(&pool)
                    .eq(BenchPgUser::ACTIVE, true)
                    .unlimited()
                    .all()
                    .await
                    .unwrap();
                for u in users {
                    let mut u = u.into_inner();
                    u.score = Some(FieldInt(7));
                    update(&pool, u).await.unwrap();
                }
            })
        })
    });

    group.finish();
}

fn bench_group_by(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(async {
        let pool = setup().await;
        let users: Vec<BenchPgUser> = (0..1000)
            .map(|i| {
                let mut user = make_user(i);
                user.active = FieldBool(i % 2 == 0);
                user
            })
            .collect();
        bulk_create(&pool, &users).await.unwrap();
        pool
    });

    let mut group = c.benchmark_group("postgres/group_by_1000");
    group.sample_size(20);

    group.bench_function("rango", |b| {
        b.iter(|| {
            rt.block_on(async {
                BenchPgUser::filter(&pool)
                    .group_by(BenchPgUser::ACTIVE)
                    .annotate_count("n")
                    .annotate_sum("total", BenchPgUser::SCORE)
                    .aggregate()
                    .await
                    .unwrap()
            })
        })
    });

    group.bench_function("naive", |b| {
        b.iter(|| {
            rt.block_on(async {
                let users = BenchPgUser::filter(&pool).unlimited().all().await.unwrap();
                let mut map: std::collections::HashMap<bool, (i64, i64)> =
                    std::collections::HashMap::new();
                for u in users {
                    let active = u.active.0;
                    let score = u.score.as_ref().map(|s| s.0 as i64).unwrap_or(0);
                    let entry = map.entry(active).or_insert((0, 0));
                    entry.0 += 1;
                    entry.1 += score;
                }
                map
            })
        })
    });

    group.finish();
}

fn bench_m2m_add_many(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(setup());
    let tags = rt.block_on(setup_m2m(&pool));

    let mut group = c.benchmark_group("postgres/m2m_add_100");
    group.sample_size(20);

    group.bench_function("rango", |b| {
        b.iter_batched(
            || {
                let blog = BenchPgBlog {
                    id: FieldUuid(Uuid::new_v4()),
                    title: FieldVarchar::new("Blog".to_string()).unwrap(),
                    tags: ManyToMany::empty(),
                };
                rt.block_on(async { insert(&pool, blog.clone()).await.unwrap() });
                blog
            },
            |blog| {
                rt.block_on(async {
                    M2M::<BenchPgBlog, BenchPgTag>::add_many(&pool, &blog, &tags)
                        .await
                        .unwrap()
                })
            },
            BatchSize::PerIteration,
        )
    });

    group.bench_function("naive", |b| {
        b.iter_batched(
            || {
                let blog = BenchPgBlog {
                    id: FieldUuid(Uuid::new_v4()),
                    title: FieldVarchar::new("Blog".to_string()).unwrap(),
                    tags: ManyToMany::empty(),
                };
                rt.block_on(async { insert(&pool, blog.clone()).await.unwrap() });
                blog
            },
            |blog| {
                rt.block_on(async {
                    for t in &tags {
                        M2M::<BenchPgBlog, BenchPgTag>::add(&pool, &blog, t)
                            .await
                            .unwrap();
                    }
                })
            },
            BatchSize::PerIteration,
        )
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_insert,
    bench_bulk_create,
    bench_filter,
    bench_aggregations,
    bench_queryset_update,
    bench_group_by,
    bench_m2m_add_many
);
criterion_main!(benches);
