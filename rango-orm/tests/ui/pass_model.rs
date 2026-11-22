use rango::{FieldText, FieldUuid, ForeignKey, ManyToMany, Model};

#[derive(Model)]
struct Tag {
    id: FieldUuid,
    name: FieldText,
}

// `Tag` is neither Clone, Default nor Debug: relations must not require it.
#[derive(Model)]
struct Article {
    id: FieldUuid,
    title: FieldText,
    summary: Option<FieldText>,
    author_id: ForeignKey<Tag>,
    tags: ManyToMany<Tag>,
}

#[derive(rango::Model)]
struct Doc {
    id: FieldUuid,
    #[field(index = "cosine")]
    embedding: rango::FieldVector<3>,
    #[field(index = "inner_product")]
    summary_embedding: Option<rango::FieldVector<3>>,
}

fn search<'a>(
    pool: &'a rango::PgPool,
    q: &rango::FieldVector<3>,
) -> rango::postgres::QueryBuilder<'a, Doc> {
    use rango::postgres::{Distance, PgQueryExt, RangoFilterExt};
    Doc::filter(pool)
        .nearest(Doc::EMBEDDING, q, 5, Distance::Cosine)
        .within(Doc::SUMMARY_EMBEDDING, q, 0.5, Distance::InnerProduct)
}

fn main() {
    let _ = search;
    let fk: ForeignKey<Tag> = ForeignKey::new(uuid::Uuid::nil());
    let _ = (fk.clone(), format!("{fk:?}"));
    let m2m: ManyToMany<Tag> = ManyToMany::default();
    let _ = (m2m.clone(), format!("{m2m:?}"));
}
