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

/// Defense in depth for column names passed as strings (`.eq("email", ..)`): an unknown
/// column becomes a typed error before any SQL runs, instead of a database error.
/// Returns the message for `Error::Invalid` so query builders can defer it to execution.
#[doc(hidden)]
pub fn unknown_column<M: Model>(name: &str) -> Option<String> {
    // ponytail: builds M::schema() per call; cache the column list if it shows up in profiles.
    if M::schema().columns.iter().any(|c| c.name == name) {
        None
    } else {
        Some(format!(
            "unknown column `{name}` on table `{}`",
            M::table_name()
        ))
    }
}
