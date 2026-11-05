use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectOptions, ConnectionTrait, Database, EntityTrait,
    IntoActiveModel, PaginatorTrait, QueryFilter, QuerySelect, Set, TransactionTrait,
};
use uuid::Uuid;

mod bench_user {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "seaorm_bench_user")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub email: String,
        pub name: String,
        pub active: bool,
        pub score: Option<i32>,
        pub created_at: ChronoDateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

async fn setup() -> sea_orm::DatabaseConnection {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5433/rango_test".to_string());

    let mut opt = ConnectOptions::new(url);
    opt.max_connections(10);
    opt.sqlx_logging(false);

    let db = Database::connect(opt)
        .await
        .expect("Failed to connect — set DATABASE_URL");

    db.execute_unprepared(
        r#"CREATE TABLE IF NOT EXISTS "seaorm_bench_user" (
            "id"         UUID PRIMARY KEY,
            "email"      VARCHAR(254) NOT NULL,
            "name"       VARCHAR(100) NOT NULL,
            "active"     BOOLEAN NOT NULL,
            "score"      INTEGER,
            "created_at" TIMESTAMPTZ NOT NULL
        )"#,
    )
    .await
    .unwrap();

    db.execute_unprepared(r#"TRUNCATE "seaorm_bench_user""#)
        .await
        .unwrap();

    db
}

fn make_user(i: usize) -> bench_user::ActiveModel {
    bench_user::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set(format!("u{}@bench.com", i)),
        name: Set("Bench User".to_string()),
        active: Set(true),
        score: Set(Some(i as i32)),
        created_at: Set(chrono::Utc::now()),
    }
}

fn bench_insert_one(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let db = rt.block_on(setup());

    c.bench_function("seaorm/insert_one", |b| {
        b.iter_batched(
            || make_user(0),
            |am| rt.block_on(async { am.insert(&db).await.unwrap() }),
            BatchSize::SmallInput,
        )
    });
}

fn bench_bulk_insert(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let db = rt.block_on(setup());
    let mut group = c.benchmark_group("seaorm/bulk_insert");

    for size in [10usize, 100, 1000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_batched(
                || (0..size).map(make_user).collect::<Vec<_>>(),
                |users| {
                    rt.block_on(async {
                        bench_user::Entity::insert_many(users.clone())
                            .exec(&db)
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

fn bench_select(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (db, first_id) = rt.block_on(async {
        let db = setup().await;
        let users: Vec<bench_user::ActiveModel> = (0..1000).map(make_user).collect();
        let inserted = bench_user::Entity::insert_many(users.clone())
            .exec_with_returning(&db)
            .await
            .unwrap();
        let first_id = inserted[0].id;
        (db, first_id)
    });

    c.bench_function("seaorm/select_pk", |b| {
        b.iter(|| {
            rt.block_on(async {
                bench_user::Entity::find_by_id(first_id)
                    .one(&db)
                    .await
                    .unwrap()
                    .unwrap()
            })
        })
    });

    c.bench_function("seaorm/select_filter_100", |b| {
        b.iter(|| {
            rt.block_on(async {
                bench_user::Entity::find()
                    .filter(bench_user::Column::Active.eq(true))
                    .limit(100)
                    .all(&db)
                    .await
                    .unwrap()
            })
        })
    });

    c.bench_function("seaorm/select_all_1000", |b| {
        b.iter(|| {
            rt.block_on(async {
                bench_user::Entity::find().all(&db).await.unwrap()
            })
        })
    });

    c.bench_function("seaorm/count", |b| {
        b.iter(|| {
            rt.block_on(async {
                bench_user::Entity::find().count(&db).await.unwrap()
            })
        })
    });
}

fn bench_update(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (db, user) = rt.block_on(async {
        let db = setup().await;
        let user = make_user(0).insert(&db).await.unwrap();
        (db, user)
    });

    c.bench_function("seaorm/update_one", |b| {
        b.iter(|| {
            let mut user = user.clone();
            user.name = "Updated".to_string();
            // reset_all: mark every column dirty → full-row UPDATE, like Rango
            let am = user.into_active_model().reset_all();
            rt.block_on(async { am.update(&db).await.unwrap() })
        })
    });
}

fn bench_transaction(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let db = rt.block_on(setup());

    c.bench_function("seaorm/transaction_10_inserts", |b| {
        b.iter(|| {
            rt.block_on(async {
                let tx = db.begin().await.unwrap();
                for i in 0..10usize {
                    make_user(i).insert(&tx).await.unwrap();
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
