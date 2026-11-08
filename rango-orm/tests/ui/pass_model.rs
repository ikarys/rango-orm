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

fn main() {
    let fk: ForeignKey<Tag> = ForeignKey::new(uuid::Uuid::nil());
    let _ = (fk.clone(), format!("{fk:?}"));
    let m2m: ManyToMany<Tag> = ManyToMany::default();
    let _ = (m2m.clone(), format!("{m2m:?}"));
}
