//! Property test: random model graphs (FK cycles, self references, nullable and NOT NULL
//! FKs, M2M pivots) must produce SQL that applies on a real database, and the snapshot of
//! those schemas must diff to nothing.
//!
//! SQLite (in memory) always runs. Postgres (`integration`, `DATABASE_URL`) and MySQL /
//! MariaDB (`integration-mysql`, `MYSQL_URL`) run about 50 seeds each, on `rg*`-prefixed
//! tables that are dropped before and after every seed.
//!
//! Deliberate limitations of the generator: no self-referencing M2M (the pivot would get two
//! columns with the same name) and M2M only between tables with a UUID primary key (the
//! pivot columns are UUID, see `generate_pivot_table`).
use crate::migration::diff::{SchemaDiff, diff};
use crate::migration::scanner::{M2MRelation, generate_pivot_table};
use crate::migration::snapshot::Snapshot;
use crate::migration::sql::generate_sql_from_diff;
use rango_orm_core::{
    BackendKind, ColumnDef, ColumnType, FkReference, ReferentialAction, TableSchema,
};

/// xorshift64, as in the fuzz tests of `rango-orm-core`.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

fn col(name: &str, col_type: ColumnType) -> ColumnDef {
    ColumnDef {
        name: name.into(),
        col_type,
        nullable: false,
        primary_key: false,
        unique: false,
        default: None,
        references: None,
        index: false,
    }
}

/// FK column type that matches the primary key type of the target.
fn fk_type(pk: &ColumnType) -> ColumnType {
    match pk {
        ColumnType::AutoBigInt => ColumnType::BigInt,
        ColumnType::AutoInt => ColumnType::Int,
        other => other.clone(),
    }
}

/// A random graph of 2..=8 tables (`rg{seed}t{i}`) plus its M2M pivots.
fn random_graph(seed: u64) -> Vec<TableSchema> {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    for _ in 0..4 {
        rng.next();
    }
    let n = 2 + rng.below(7);
    let name = |i: usize| format!("rg{seed}t{i}");
    let pks: Vec<ColumnType> = (0..n)
        .map(|_| match rng.below(3) {
            0 => ColumnType::Uuid,
            1 => ColumnType::AutoBigInt,
            _ => ColumnType::AutoInt,
        })
        .collect();

    let mut tables: Vec<TableSchema> = Vec::new();
    for i in 0..n {
        let mut id = col("id", pks[i].clone());
        id.primary_key = true;
        let mut columns = vec![id];
        for k in 0..rng.below(3) {
            let ty = match rng.below(4) {
                0 => ColumnType::Text,
                1 => ColumnType::Int,
                2 => ColumnType::Varchar(50),
                _ => ColumnType::Bool,
            };
            let mut c = col(&format!("c{k}"), ty);
            c.nullable = rng.chance(50);
            c.index = rng.chance(20);
            c.unique = rng.chance(10) && matches!(c.col_type, ColumnType::Int);
            columns.push(c);
        }
        // Any table may point at any table (itself included), so cycles are common.
        for k in 0..rng.below(4) {
            let target = rng.below(n);
            let nullable = rng.chance(50);
            let on_delete = match rng.below(4) {
                0 => ReferentialAction::Cascade,
                1 if nullable => ReferentialAction::SetNull,
                2 => ReferentialAction::Restrict,
                _ => ReferentialAction::NoAction,
            };
            let mut c = col(&format!("fk{k}"), fk_type(&pks[target]));
            c.nullable = nullable;
            c.index = true;
            c.references = Some(FkReference {
                table: name(target),
                column: "id".into(),
                on_delete,
                on_update: ReferentialAction::NoAction,
            });
            columns.push(c);
        }
        tables.push(TableSchema {
            table_name: name(i),
            columns,
            ..Default::default()
        });
    }

    let uuid_tables: Vec<usize> = (0..n).filter(|&i| pks[i] == ColumnType::Uuid).collect();
    let mut seen = std::collections::HashSet::new();
    for _ in 0..rng.below(3) {
        if uuid_tables.len() < 2 {
            break;
        }
        let (a, b) = (
            uuid_tables[rng.below(uuid_tables.len())],
            uuid_tables[rng.below(uuid_tables.len())],
        );
        if a != b && seen.insert((a, b)) {
            tables.push(generate_pivot_table(&M2MRelation {
                from_table: name(a),
                to_table: name(b),
            }));
        }
    }
    tables
}

/// A ring of `n` tables where every FK is NOT NULL.
fn not_null_ring(n: usize) -> Vec<TableSchema> {
    (0..n)
        .map(|i| {
            let mut id = col("id", ColumnType::Uuid);
            id.primary_key = true;
            let mut fk = col("next_id", ColumnType::Uuid);
            fk.references = Some(FkReference {
                table: format!("rgring{n}t{}", (i + 1) % n),
                column: "id".into(),
                on_delete: ReferentialAction::NoAction,
                on_update: ReferentialAction::NoAction,
            });
            TableSchema {
                table_name: format!("rgring{n}t{i}"),
                columns: vec![id, fk],
                ..Default::default()
            }
        })
        .collect()
}

fn creates(schemas: &[TableSchema]) -> Vec<SchemaDiff> {
    diff(&Snapshot::default(), schemas)
}

/// The snapshot of `schemas` describes exactly `schemas`: nothing left to migrate.
fn assert_snapshot_clean(schemas: &[TableSchema], what: &str) {
    let snap = Snapshot::from_schemas(schemas);
    let left = diff(&snap, schemas);
    assert!(left.is_empty(), "{what}: diff not empty: {left:?}");
    let back = snap.to_schemas().unwrap();
    let left = diff(&Snapshot::from_schemas(&back), schemas);
    assert!(left.is_empty(), "{what}: round trip diff: {left:?}");
}

fn sql_for(schemas: &[TableSchema], backend: BackendKind) -> String {
    generate_sql_from_diff(&creates(schemas), backend)
}

fn all_graphs(seeds: u64) -> Vec<(String, Vec<TableSchema>)> {
    let mut v: Vec<_> = (1..=seeds)
        .map(|s| (format!("seed {s}"), random_graph(s)))
        .collect();
    v.push(("ring2".into(), not_null_ring(2)));
    v.push(("ring3".into(), not_null_ring(3)));
    v.push(("ring5".into(), not_null_ring(5)));
    v
}

#[test]
fn generator_is_deterministic_and_covers_cycles() {
    assert_eq!(
        format!("{:?}", random_graph(7)),
        format!("{:?}", random_graph(7))
    );
    // Some seed has an FK cycle and a NOT NULL FK, otherwise the property proves little.
    let graphs: Vec<_> = (1..=50).map(random_graph).collect();
    assert!(graphs.iter().any(|g| {
        g.iter().any(|t| {
            t.columns.iter().any(|c| {
                c.references
                    .as_ref()
                    .is_some_and(|r| r.table == t.table_name)
            })
        })
    }));
    assert!(graphs.iter().flatten().any(|t| {
        t.columns
            .iter()
            .any(|c| c.references.is_some() && !c.nullable)
    }));
    assert!(graphs.iter().any(|g| {
        g.iter()
            .any(|t| t.columns.len() == 2 && t.columns.iter().all(|c| c.references.is_some()))
    }));
}

#[test]
fn snapshots_of_random_graphs_are_clean() {
    for (what, g) in all_graphs(200) {
        assert_snapshot_clean(&g, &what);
    }
}

#[tokio::test]
async fn random_graphs_apply_on_sqlite() {
    use sqlx::sqlite::SqlitePoolOptions;
    for (what, g) in all_graphs(200) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let sql = sql_for(&g, BackendKind::Sqlite);
        sqlx::raw_sql(&sql)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("{what}: {e}\n{sql}"));
        pool.close().await;
    }
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn random_graphs_apply_on_postgres() {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5433/rango_test".into());
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    for (what, g) in all_graphs(50) {
        let drop = || {
            g.iter()
                .map(|t| format!("DROP TABLE IF EXISTS \"{}\" CASCADE;", t.table_name))
                .collect::<String>()
        };
        sqlx::raw_sql(&drop()).execute(&pool).await.unwrap();
        let sql = sql_for(&g, BackendKind::Postgres);
        let res = sqlx::raw_sql(&sql).execute(&pool).await;
        sqlx::raw_sql(&drop()).execute(&pool).await.unwrap();
        res.unwrap_or_else(|e| panic!("{what}: {e}\n{sql}"));
    }
    pool.close().await;
}

#[cfg(feature = "integration-mysql")]
#[tokio::test]
async fn random_graphs_apply_on_mysql() {
    let url = std::env::var("MYSQL_URL")
        .unwrap_or_else(|_| "mysql://root:rango@127.0.0.1:3307/rango_test".into());
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    for (what, g) in all_graphs(50) {
        let drop = || {
            let drops: String = g
                .iter()
                .map(|t| format!("DROP TABLE IF EXISTS `{}`;", t.table_name))
                .collect();
            format!("SET FOREIGN_KEY_CHECKS = 0; {drops} SET FOREIGN_KEY_CHECKS = 1;")
        };
        sqlx::raw_sql(&drop()).execute(&pool).await.unwrap();
        let sql = sql_for(&g, BackendKind::Mysql);
        let res = sqlx::raw_sql(&sql).execute(&pool).await;
        sqlx::raw_sql(&drop()).execute(&pool).await.unwrap();
        res.unwrap_or_else(|e| panic!("{what}: {e}\n{sql}"));
    }
    pool.close().await;
}
