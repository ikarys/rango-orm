use rango::{Model, ModelValues};

struct Plain;

fn needs_model<M: Model>() {}
fn needs_values<M: ModelValues>() {}

fn main() {
    needs_model::<Plain>();
    needs_values::<Plain>();
}
