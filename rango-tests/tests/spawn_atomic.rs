//! Compile-only: `atomic` and `atomic_retry` futures must be spawnable.
#![allow(dead_code)]

use rango_orm_core::Result;

fn spawn_pg(pool: sqlx::PgPool) {
    let pool2 = pool.clone();
    tokio::spawn(async move {
        rango_orm_postgres::atomic_retry(&pool, 6, async |tx| -> Result<()> {
            rango_orm_postgres::raw_execute(&mut **tx, "SELECT 1", vec![]).await?;
            Ok(())
        })
        .await
    });
    tokio::spawn(async move {
        rango_orm_postgres::atomic(&pool2, async |tx| -> Result<()> {
            rango_orm_postgres::raw_execute(&mut **tx, "SELECT 1", vec![]).await?;
            Ok(())
        })
        .await
    });
}

fn spawn_mysql(pool: sqlx::MySqlPool) {
    tokio::spawn(async move {
        rango_orm_mysql::atomic_retry(&pool, 6, async |tx| -> Result<()> {
            rango_orm_mysql::raw_execute(&mut **tx, "SELECT 1", vec![]).await?;
            Ok(())
        })
        .await
    });
}

use rango_orm_core::*;
use rango_orm_derive::Model;
use rango_orm_postgres::RangoFilterExt;

#[derive(Model, Debug, Clone)]
#[model(table = "spawn_item")]
struct Item {
    id: FieldUuid,
    name: FieldVarchar<1, 100>,
}

fn spawn_pg_models(pool: sqlx::PgPool, name: String) {
    tokio::spawn(async move {
        let name = &name;
        rango_orm_postgres::atomic_retry(&pool, 6, async |tx| {
            let n = rango_orm_postgres::raw_scalar::<_, i64>(&mut **tx, "SELECT 1", vec![]).await?;
            let it = Item {
                id: FieldUuid::default(),
                name: FieldVarchar::new(name.clone()).unwrap(),
            };
            rango_orm_postgres::insert(&mut **tx, it).await?;
            let all = Item::filter(&mut **tx).all().await?;
            Ok((n, all.len()))
        })
        .await
    });
}

#[test]
fn compiles() {}
