use rango::prelude::*;

#[derive(Model)]
struct Doc {
    id: FieldUuid,
    embedding: FieldVector<3>,
}

fn search<'a>(pool: &'a PgPool, q: &FieldVector<4>) -> QueryBuilder<'a, Doc> {
    Doc::filter(pool).within(Doc::EMBEDDING, q, 0.5, Distance::L2)
}

fn main() {}
