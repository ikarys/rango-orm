use rango::prelude::*;

#[derive(Model)]
struct Doc {
    id: FieldUuid,
    title: FieldText,
    embedding: FieldVector<3>,
}

fn search<'a>(pool: &'a PgPool, q: &FieldVector<3>) -> QueryBuilder<'a, Doc> {
    Doc::filter(pool).nearest(Doc::TITLE, q, 5, Distance::Cosine)
}

fn main() {}
