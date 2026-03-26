
/// Error when reading a row.
#[derive(Debug)]
pub struct RowError(pub String);

impl std::fmt::Display for RowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for RowError {}

/// Backend-agnostic row abstraction.
/// Implemented by rango-postgres (and future backends) for their native row type.
pub trait RangoRow {
    fn get_bool(&self, col: &str) -> Result<bool, RowError>;
    fn get_i16(&self, col: &str) -> Result<i16, RowError>;
    fn get_i32(&self, col: &str) -> Result<i32, RowError>;
    fn get_i64(&self, col: &str) -> Result<i64, RowError>;
    fn get_f32(&self, col: &str) -> Result<f32, RowError>;
    fn get_f64(&self, col: &str) -> Result<f64, RowError>;
    fn get_string(&self, col: &str) -> Result<String, RowError>;
    fn get_bytes(&self, col: &str) -> Result<Vec<u8>, RowError>;
    fn get_uuid(&self, col: &str) -> Result<uuid::Uuid, RowError>;
    fn get_datetime(&self, col: &str) -> Result<chrono::DateTime<chrono::Utc>, RowError>;
    fn get_date(&self, col: &str) -> Result<chrono::NaiveDate, RowError>;
    fn get_time(&self, col: &str) -> Result<chrono::NaiveTime, RowError>;
    fn get_json(&self, col: &str) -> Result<serde_json::Value, RowError>;
    fn is_null(&self, col: &str) -> bool;
}

/// Implemented by `#[derive(Model)]` — reads a model from a RangoRow.
pub trait FromRow: Sized {
    fn from_row(row: &dyn RangoRow) -> Result<Self, RowError>;
}
