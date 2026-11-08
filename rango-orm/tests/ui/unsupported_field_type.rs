use rango::{FieldUuid, Model};

#[derive(Model)]
struct User {
    id: FieldUuid,
    name: String,
}

fn main() {}
