use rango::prelude::*;

#[derive(Model)]
struct HnswOnText {
    id: FieldUuid,
    #[field(index = "cosine")]
    title: FieldText,
}

#[derive(Model)]
struct BtreeOnVector {
    id: FieldUuid,
    #[field(index)]
    embedding: FieldVector<3>,
}

#[derive(Model)]
struct UnknownDistance {
    id: FieldUuid,
    #[field(index = "euclid")]
    embedding: FieldVector<3>,
}

fn main() {}
