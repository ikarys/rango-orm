use rango_core::{FromRow, RangoRow, RowError};
use sqlx::{mysql::MySqlRow, Row, ValueRef};

pub struct MySqlRangoRow(pub MySqlRow);

impl RangoRow for MySqlRangoRow {
    fn get_bool(&self, col: &str) -> Result<bool, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_i16(&self, col: &str) -> Result<i16, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_i32(&self, col: &str) -> Result<i32, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_i64(&self, col: &str) -> Result<i64, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_f32(&self, col: &str) -> Result<f32, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_f64(&self, col: &str) -> Result<f64, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_string(&self, col: &str) -> Result<String, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_bytes(&self, col: &str) -> Result<Vec<u8>, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_uuid(&self, col: &str) -> Result<uuid::Uuid, RowError> {
        let b: Vec<u8> = self
            .0
            .try_get(col)
            .map_err(|e| RowError(format!("{}: {}", col, e)))?;
        uuid::Uuid::from_slice(&b).map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_datetime(&self, col: &str) -> Result<chrono::DateTime<chrono::Utc>, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_date(&self, col: &str) -> Result<chrono::NaiveDate, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_time(&self, col: &str) -> Result<chrono::NaiveTime, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn get_json(&self, col: &str) -> Result<serde_json::Value, RowError> {
        // MariaDB's JSON is a LONGTEXT alias: decode as text, which also works on MySQL.
        let raw = self
            .0
            .try_get_unchecked::<String, _>(col)
            .map_err(|e| RowError(format!("{}: {}", col, e)))?;
        serde_json::from_str(&raw).map_err(|e| RowError(format!("{}: {}", col, e)))
    }
    fn is_null(&self, col: &str) -> bool {
        match self.0.try_get_raw(col) {
            Ok(value) => value.is_null(),
            Err(_) => false,
        }
    }
}

/// Decode every row into `M`, allocating the result once.
/// (Collecting an iterator of `Result` loses the size hint and regrows the Vec.)
pub(crate) fn decode_all<M: FromRow>(rows: Vec<MySqlRow>) -> rango_core::Result<Vec<M>> {
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(M::from_row(&MySqlRangoRow(r))?);
    }
    Ok(out)
}
