use rango::{FieldUuid, Model};

#[derive(Model)]
struct Blob {
    id: FieldUuid,
    data: Option<Vec<u8>>,
}

fn main() {}
