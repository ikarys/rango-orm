/// A SQL-compatible value — used to bind field values to queries
/// without requiring sqlx as a direct dependency of rango-core.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    // Typed nulls — carry the column type so the DB can infer correctly.
    NullBool,
    NullSmallInt,
    NullInt,
    NullBigInt,
    NullFloat,
    NullDouble,
    NullText,
    NullBytes,
    NullUuid,
    NullDateTime,
    NullDate,
    NullTime,
    NullJson,
    // Non-null values
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

// ─── From<T> conversions for SqlValue ────────────────────────────────────────

impl From<&str> for SqlValue {
    fn from(s: &str) -> Self {
        SqlValue::Text(s.to_string())
    }
}
impl From<String> for SqlValue {
    fn from(s: String) -> Self {
        SqlValue::Text(s)
    }
}
impl From<bool> for SqlValue {
    fn from(v: bool) -> Self {
        SqlValue::Bool(v)
    }
}
impl From<i16> for SqlValue {
    fn from(v: i16) -> Self {
        SqlValue::SmallInt(v)
    }
}
impl From<i32> for SqlValue {
    fn from(v: i32) -> Self {
        SqlValue::Int(v)
    }
}
impl From<i64> for SqlValue {
    fn from(v: i64) -> Self {
        SqlValue::BigInt(v)
    }
}
impl From<f32> for SqlValue {
    fn from(v: f32) -> Self {
        SqlValue::Float(v)
    }
}
impl From<f64> for SqlValue {
    fn from(v: f64) -> Self {
        SqlValue::Double(v)
    }
}
impl From<uuid::Uuid> for SqlValue {
    fn from(v: uuid::Uuid) -> Self {
        SqlValue::Uuid(v)
    }
}
impl From<chrono::DateTime<chrono::Utc>> for SqlValue {
    fn from(v: chrono::DateTime<chrono::Utc>) -> Self {
        SqlValue::DateTime(v)
    }
}
impl From<Vec<u8>> for SqlValue {
    fn from(v: Vec<u8>) -> Self {
        SqlValue::Bytes(v)
    }
}
impl From<serde_json::Value> for SqlValue {
    fn from(v: serde_json::Value) -> Self {
        SqlValue::Json(v)
    }
}

/// Marker trait — implemented by #[derive(Model)] to enable filter() method.
/// The actual QueryBuilder is in rango-postgres to avoid circular deps.
pub trait Filterable: Sized {}

/// Implemented by `#[derive(Model)]` — provides field names + values for INSERT/UPDATE.
pub trait ModelValues {
    /// Returns (column_name, value) pairs for all non-PK fields.
    fn field_values(&self) -> Vec<(&'static str, SqlValue)>;

    /// Returns the primary key value.
    fn pk_value(&self) -> SqlValue;

    /// Returns the primary key column name.
    fn pk_column() -> &'static str;
}
