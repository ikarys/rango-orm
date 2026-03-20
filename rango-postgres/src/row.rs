use rango_core::row::{RangoRow, RowError};
use sqlx::postgres::PgRow;
use sqlx::Row;

pub struct PgRangoRow(pub PgRow);

macro_rules! impl_get {
    ($fn_name:ident, $t:ty) => {
        fn $fn_name(&self, col: &str) -> Result<$t, RowError> {
            self.0.try_get::<$t, _>(col)
                .map_err(|e| RowError(format!("Column '{}': {}", col, e)))
        }
    };
}

impl RangoRow for PgRangoRow {
    impl_get!(get_bool, bool);
    impl_get!(get_i16, i16);
    impl_get!(get_i32, i32);
    impl_get!(get_i64, i64);
    impl_get!(get_f32, f32);
    impl_get!(get_f64, f64);
    impl_get!(get_string, String);
    impl_get!(get_bytes, Vec<u8>);
    impl_get!(get_uuid, uuid::Uuid);
    impl_get!(get_datetime, chrono::DateTime<chrono::Utc>);
    impl_get!(get_date, chrono::NaiveDate);
    impl_get!(get_time, chrono::NaiveTime);

    fn get_json(&self, col: &str) -> Result<serde_json::Value, RowError> {
        self.0.try_get::<sqlx::types::Json<serde_json::Value>, _>(col)
            .map(|j| j.0)
            .map_err(|e| RowError(format!("Column '{}': {}", col, e)))
    }

    fn is_null(&self, col: &str) -> bool {
        self.0.try_get::<Option<String>, _>(col)
            .map(|v| v.is_none())
            .unwrap_or(true)
    }
}
