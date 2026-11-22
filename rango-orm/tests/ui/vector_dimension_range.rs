use rango::prelude::*;

#[derive(Model)]
struct TooWide {
    id: FieldUuid,
    embedding: FieldVector<16001>,
}

fn main() {}
