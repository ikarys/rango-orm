use crate::schema::TableSchema;

/// Core trait implemented by `#[derive(Model)]`.
/// Every model must be able to describe its own schema.
pub trait Model {
    /// The table name in the database.
    fn table_name() -> &'static str;

    /// The full schema description for this model.
    fn schema() -> TableSchema;
}

/// Marker trait for abstract model mixins (`#[derive(ModelMixin)]`).
/// A mixin is never a real table — its fields get flattened into models that use it.
pub trait ModelMixin {
    fn mixin_name() -> &'static str;
}
