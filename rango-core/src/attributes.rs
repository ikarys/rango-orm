/// Parsed representation of `#[field(...)]` attributes on a model field.
/// Used by rango-derive to generate the correct ColumnDef.
///
/// Example:
/// ```rust
/// #[field(unique, index)]
/// email: FieldEmail,
///
/// #[field(default = "now()")]
/// created_at: FieldDateTime,
/// ```
#[derive(Debug, Default, Clone)]
pub struct FieldAttributes {
    /// Mark the column as UNIQUE in the DB.
    pub unique: bool,

    /// Create a DB index on this column.
    pub index: bool,

    /// Override the column name (default: snake_case of the field name).
    pub column_name: Option<String>,

    /// SQL default expression (e.g. "now()", "0", "'active'").
    pub default: Option<String>,

    /// Auto-set to current timestamp on insert.
    pub auto_now_add: bool,

    /// Auto-set to current timestamp on insert and update.
    pub auto_now: bool,
}
