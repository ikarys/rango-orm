/// sqlx raw baseline — zero ORM overhead.
/// Same workloads as bench-rango for direct comparison.
///
/// Run: DATABASE_URL=postgres://... cargo bench -p bench-sqlx-raw
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

type BenchRow = (
    Uuid,
    String,
    String,
    bool,
    Option<i32>,
    chrono::DateTime<chrono::Utc>,
);

// ── Setup ─────────────────────────────────────────────────────────────────────

async fn setup() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5433/rango_test".to_string());
    let pool = PgPool::connect(&url)
        .await
        .expect("Failed to connect — set DATABASE_URL");
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS "sqlx_bench_user" (
            "id"       UUID PRIMARY KEY,
            "email"    VARCHAR(254) NOT NULL,
            "name"     VARCHAR(100) NOT NULL,
            "active"   BOOLEAN NOT NULL,
            "score"    INTEGER,
            "created_at" TIMESTAMPTZ NOT NULL
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(r#"TRUNCATE "sqlx_bench_user""#)
        .execute(&pool)
        .await
        .unwrap();
    pool
}

fn user_id() -> Uuid {
    Uuid::new_v4()
}
fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}

// ── insert_one ────────────────────────────────────────────────────────────────

fn bench_insert_one(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(setup());

    c.bench_function("sqlx_raw/insert_one", |b| {
        b.iter(|| {
            rt.block_on(async {
                sqlx::query(
                    r#"INSERT INTO "sqlx_bench_user"
                       ("id","email","name","active","score","created_at")
                       VALUES ($1,$2,$3,$4,$5,$6) RETURNING *"#,
                )
                .bind(user_id())
                .bind("bench@example.com")
                .bind("Bench User")
                .bind(true)
                .bind(Option::<i32>::None)
                .bind(now())
                .fetch_one(&pool)
                .await
                .unwrap()
            })
        })
    });
}

// ── bulk_insert ───────────────────────────────────────────────────────────────

fn bench_bulk_insert(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(setup());
    let mut group = c.benchmark_group("sqlx_raw/bulk_insert");

    for size in [10usize, 100, 1000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_batched(
                || (0..size).map(|i| (Uuid::new_v4(), format!("u{}@bench.com", i), i as i32)).collect::<Vec<_>>(),
                |rows| {
                    rt.block_on(async {
                        // Use UNNEST for bulk insert — proper baseline
                        let ids: Vec<Uuid> = rows.iter().map(|(id, _, _)| *id).collect();
                        let emails: Vec<&str> = rows.iter().map(|(_, e, _)| e.as_str()).collect();
                        let scores: Vec<i32> = rows.iter().map(|(_, _, s)| *s).collect();
                        sqlx::query(
                            r#"INSERT INTO "sqlx_bench_user" ("id","email","name","active","score","created_at")
                               SELECT * FROM UNNEST($1::uuid[], $2::text[], $3::text[], $4::bool[], $5::int4[], $6::timestamptz[])"#,
                        )
                        .bind(&ids)
                        .bind(&emails)
                        .bind(vec!["Bench User"; rows.len()])
                        .bind(vec![true; rows.len()])
                        .bind(&scores)
                        .bind(vec![chrono::Utc::now(); rows.len()])
                        .execute(&pool)
                        .await
                        .unwrap()
                    })
                },
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

// ── select_pk ─────────────────────────────────────────────────────────────────

fn bench_select(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (pool, first_id) = rt.block_on(async {
        let pool = setup().await;
        // Insert 1000 rows
        for i in 0..1000usize {
            sqlx::query(
                r#"INSERT INTO "sqlx_bench_user"
                   ("id","email","name","active","score","created_at")
                   VALUES ($1,$2,$3,$4,$5,now())"#,
            )
            .bind(Uuid::new_v4())
            .bind(format!("u{}@bench.com", i))
            .bind("User")
            .bind(true)
            .bind(i as i32)
            .execute(&pool)
            .await
            .unwrap();
        }
        // Get first id
        let row: (Uuid,) = sqlx::query_as(r#"SELECT "id" FROM "sqlx_bench_user" LIMIT 1"#)
            .fetch_one(&pool)
            .await
            .unwrap();
        (pool, row.0)
    });

    c.bench_function("sqlx_raw/select_pk", |b| {
        b.iter(|| {
            rt.block_on(async {
                sqlx::query(r#"SELECT * FROM "sqlx_bench_user" WHERE "id" = $1 LIMIT 1"#)
                    .bind(first_id)
                    .fetch_optional(&pool)
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("sqlx_raw/select_filter_100", |b| {
        b.iter(|| {
            rt.block_on(async {
                sqlx::query(r#"SELECT * FROM "sqlx_bench_user" WHERE "active" = $1 LIMIT 100"#)
                    .bind(true)
                    .fetch_all(&pool)
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("sqlx_raw/select_all_1000", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Decoded by column index into owned values — the hand-written best case.
                sqlx::query_as::<_, BenchRow>(r#"SELECT "id","email","name","active","score","created_at" FROM "sqlx_bench_user" LIMIT 1000"#)
                    .fetch_all(&pool)
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("sqlx_raw/count", |b| {
        b.iter(|| {
            rt.block_on(async {
                sqlx::query(r#"SELECT COUNT(*) FROM "sqlx_bench_user""#)
                    .fetch_one(&pool)
                    .await
                    .unwrap()
            })
        })
    });
}

// ── update_one ────────────────────────────────────────────────────────────────

fn bench_update(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (pool, first_id) = rt.block_on(async {
        let pool = setup().await;
        sqlx::query(
            r#"INSERT INTO "sqlx_bench_user" ("id","email","name","active","score","created_at")
               VALUES ($1,'u@bench.com','User',$2,0,now())"#,
        )
        .bind(Uuid::new_v4())
        .bind(true)
        .execute(&pool)
        .await
        .unwrap();
        let row: (Uuid,) = sqlx::query_as(r#"SELECT "id" FROM "sqlx_bench_user" LIMIT 1"#)
            .fetch_one(&pool)
            .await
            .unwrap();
        (pool, row.0)
    });

    c.bench_function("sqlx_raw/update_one", |b| {
        b.iter(|| {
            rt.block_on(async {
                sqlx::query(
                    r#"UPDATE "sqlx_bench_user" SET "name" = $1 WHERE "id" = $2 RETURNING *"#,
                )
                .bind("Updated")
                .bind(first_id)
                .fetch_one(&pool)
                .await
                .unwrap()
            })
        })
    });
}

// ── transaction ───────────────────────────────────────────────────────────────

fn bench_transaction(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(setup());

    c.bench_function("sqlx_raw/transaction_10_inserts", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut tx = pool.begin().await.unwrap();
                for i in 0..10usize {
                    sqlx::query(
                        r#"INSERT INTO "sqlx_bench_user"
                           ("id","email","name","active","score","created_at")
                           VALUES ($1,$2,$3,$4,$5,now())"#,
                    )
                    .bind(Uuid::new_v4())
                    .bind(format!("tx{}@bench.com", i))
                    .bind("User")
                    .bind(true)
                    .bind(i as i32)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
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
    bench_vector,
);
criterion_main!(benches);
// ── pgvector ──────────────────────────────────────────────────────────────────
// Same data as bench-rango: 10k deterministic unit vectors of 384 dimensions, cosine distance.

const DOCS: usize = 10_000;
const DIM: usize = 384;

/// xorshift64, identical to bench-rango's.
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
    let (have, exact): (i64, i64) = sqlx::query_as(
        r#"SELECT (SELECT count(*) FROM "bench_doc"), (SELECT count(*) FROM "bench_doc_exact")"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    if have != DOCS as i64 || exact != DOCS as i64 {
        sqlx::query(r#"TRUNCATE "bench_doc", "bench_doc_exact""#)
            .execute(&pool)
            .await
            .unwrap();
        for chunk in (0..DOCS as i64).collect::<Vec<_>>().chunks(1000) {
            let vecs: Vec<Vector> = chunk
                .iter()
                .map(|&i| Vector::from(unit_vector(i as u64)))
                .collect();
            sqlx::query(
                r#"INSERT INTO "bench_doc" SELECT * FROM UNNEST($1::int8[], $2::vector[])"#,
            )
            .bind(chunk)
            .bind(vecs)
            .execute(&pool)
            .await
            .unwrap();
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

async fn nearest(pool: &PgPool, table: &'static str, q: &Vector) -> Vec<(i64, Vec<f32>)> {
    let sql = match table {
        "bench_doc" => {
            r#"SELECT "id", "embedding" FROM "bench_doc" ORDER BY "embedding" <=> $1 LIMIT 10"#
        }
        _ => {
            r#"SELECT "id", "embedding" FROM "bench_doc_exact" ORDER BY "embedding" <=> $1 LIMIT 10"#
        }
    };
    sqlx::query_as::<_, (i64, Vector)>(sql)
        .bind(q)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|(id, v)| (id, v.to_vec()))
        .collect()
}

fn bench_vector(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(vector_setup());
    let query = Vector::from(unit_vector(DOCS as u64 + 1));

    let mut group = c.benchmark_group("sqlx_raw/vector_nearest_10");
    group.bench_function("hnsw", |b| {
        b.iter(|| rt.block_on(nearest(&pool, "bench_doc", &query)))
    });
    group.bench_function("exact", |b| {
        b.iter(|| rt.block_on(nearest(&pool, "bench_doc_exact", &query)))
    });
    group.finish();

    c.bench_function("sqlx_raw/vector_bulk_insert/100", |b| {
        b.iter_batched(
            || {
                let ids: Vec<Uuid> = (0..100).map(|_| Uuid::new_v4()).collect();
                let vecs: Vec<Vector> = (0..100u64).map(|i| Vector::from(unit_vector(i))).collect();
                (ids, vecs)
            },
            |(ids, vecs)| {
                rt.block_on(async {
                    sqlx::query(r#"INSERT INTO "bench_doc_ins" SELECT * FROM UNNEST($1::uuid[], $2::vector[])"#)
                        .bind(ids)
                        .bind(vecs)
                        .execute(&pool)
                        .await
                        .unwrap()
                })
            },
            BatchSize::SmallInput,
        )
    });
}
