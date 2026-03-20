/// A SQL-compatible value — used to bind field values to queries
/// without requiring sqlx as a direct dependency of rango-core.
#[derive(Debug, Clone)]
pub enum SqlValue {
    Null,
    Bool(bool),
    SmallInt(i16),
    Int(i32),
    BigInt(i64),
    Float(f32),
    Double(f64),
    Text(String),
    Bytes(Vec<u8>),
    Uuid(uuid::Uuid),
    DateTime(chrono::DateTime<chrono::Utc>),
    Date(chrono::NaiveDate),
    Time(chrono::NaiveTime),
    Json(serde_json::Value),
}

/// Implemented by field types — converts to a SQL-bindable value.
pub trait ToSqlValue {
    fn to_sql_value(&self) -> SqlValue;
}

/// Implemented by `#[derive(Model)]` — provides field names + values for INSERT/UPDATE.
pub trait ModelValues {
    /// Returns (column_name, value) pairs for all non-PK fields.
    fn field_values(&self) -> Vec<(&'static str, SqlValue)>;

    /// Returns the primary key value.
    fn pk_value(&self) -> SqlValue;

    /// Returns the primary key column name.
    fn pk_column() -> &'static str;
}
