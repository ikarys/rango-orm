//! With every backend enabled, `use rango::prelude::*` must compile (no ambiguous names)
//! and the backends' own ops stay reachable by path.
#![cfg(all(feature = "postgres", feature = "sqlite", feature = "mysql"))]

use rango::prelude::*;

#[derive(Model)]
pub struct Item {
    pub id: FieldUuid,
    pub name: FieldVarchar<1, 100>,
}

// Prelude ops come from Postgres (feature priority).
fn _pg(pool: &PgPool) -> QueryBuilder<'_, Item> {
    Item::filter(pool).eq(Item::NAME, "x")
}

// The other backends are reachable by path; their pool types are in the prelude.
fn _sqlite(pool: &SqlitePool) -> rango::sqlite::QueryBuilder<'_, Item> {
    // Two `filter` traits would be ambiguous: name the backend's one.
    <Item as rango::sqlite::RangoFilterExt>::filter(pool).eq(Item::NAME, "x")
}

fn _mysql(pool: &MySqlPool) -> rango::mysql::QueryBuilder<'_, Item> {
    // Two `filter` traits would be ambiguous: name the backend's one.
    <Item as rango::mysql::RangoFilterExt>::filter(pool).eq(Item::NAME, "x")
}

#[test]
fn prelude_compiles_with_all_backends() {}
