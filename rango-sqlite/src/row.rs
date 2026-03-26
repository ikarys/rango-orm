use rango_core::{RangoRow, RowError};
use sqlx::{Row, sqlite::SqliteRow};

pub struct SqliteRangoRow(pub SqliteRow);

impl RangoRow for SqliteRangoRow {
    fn get_bool(&self, col: &str) -> Result<bool, RowError> {
        self.0.try_get::<i64, _>(col)
            .map(|v| v != 0)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_i16(&self, col: &str) -> Result<i16, RowError> {
        self.0.try_get::<i64, _>(col)
            .map(|v| v as i16)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_i32(&self, col: &str) -> Result<i32, RowError> {
        self.0.try_get::<i64, _>(col)
            .map(|v| v as i32)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_i64(&self, col: &str) -> Result<i64, RowError> {
        self.0.try_get(col).map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_f32(&self, col: &str) -> Result<f32, RowError> {
        self.0.try_get::<f64, _>(col)
            .map(|v| v as f32)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_f64(&self, col: &str) -> Result<f64, RowError> {
        self.0.try_get(col).map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_string(&self, col: &str) -> Result<String, RowError> {
        self.0.try_get(col).map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_bytes(&self, col: &str) -> Result<Vec<u8>, RowError> {
        self.0.try_get(col).map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_uuid(&self, col: &str) -> Result<uuid::Uuid, RowError> {
        let s: String = self.0.try_get(col).map_err(|e| RowError(format!("{}: {}", col, e)))?;
        s.parse::<uuid::Uuid>().map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_datetime(&self, col: &str) -> Result<chrono::DateTime<chrono::Utc>, RowError> {
        let s: String = self.0.try_get(col).map_err(|e| RowError(format!("{}: {}", col, e)))?;
        s.parse::<chrono::DateTime<chrono::Utc>>()
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_date(&self, col: &str) -> Result<chrono::NaiveDate, RowError> {
        let s: String = self.0.try_get(col).map_err(|e| RowError(format!("{}: {}", col, e)))?;
        s.parse::<chrono::NaiveDate>().map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_time(&self, col: &str) -> Result<chrono::NaiveTime, RowError> {
        let s: String = self.0.try_get(col).map_err(|e| RowError(format!("{}: {}", col, e)))?;
        s.parse::<chrono::NaiveTime>().map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_json(&self, col: &str) -> Result<serde_json::Value, RowError> {
        let s: String = self.0.try_get(col).map_err(|e| RowError(format!("{}: {}", col, e)))?;
        serde_json::from_str(&s).map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn is_null(&self, col: &str) -> bool {
        self.0.try_get::<Option<String>, _>(col)
            .map(|v| v.is_none())
            .unwrap_or(true)
    }
}
