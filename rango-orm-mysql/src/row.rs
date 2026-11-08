use rango_orm_core::{FromRow, RangoRow, RowError};
use sqlx::{mysql::MySqlRow, Row, ValueRef};

/// Owned row — used for simple queries.
pub struct MySqlRangoRow(pub MySqlRow);

/// Borrowed row — used for JOIN queries where we need to read the same row twice.
pub struct MySqlRowRef<'a>(pub &'a MySqlRow);

impl RangoRow for MySqlRowRef<'_> {
    fn get_bool(&self, col: &str) -> Result<bool, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_i16(&self, col: &str) -> Result<i16, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_i32(&self, col: &str) -> Result<i32, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_i64(&self, col: &str) -> Result<i64, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_f32(&self, col: &str) -> Result<f32, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_f64(&self, col: &str) -> Result<f64, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_decimal(&self, col: &str) -> Result<rango_orm_core::Decimal, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_string(&self, col: &str) -> Result<String, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_bytes(&self, col: &str) -> Result<Vec<u8>, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_uuid(&self, col: &str) -> Result<uuid::Uuid, RowError> {
        let b: Vec<u8> = self
            .0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))?;
        uuid::Uuid::from_slice(&b).map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_datetime(&self, col: &str) -> Result<chrono::DateTime<chrono::Utc>, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_date(&self, col: &str) -> Result<chrono::NaiveDate, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_time(&self, col: &str) -> Result<chrono::NaiveTime, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_json(&self, col: &str) -> Result<serde_json::Value, RowError> {
        // MariaDB's JSON is a LONGTEXT alias: decode as text, which also works on MySQL.
        let raw = self
            .0
            .try_get_unchecked::<String, _>(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))?;
        serde_json::from_str(&raw).map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn is_null(&self, col: &str) -> bool {
        match self.0.try_get_raw(col) {
            Ok(value) => value.is_null(),
            Err(_) => false,
        }
    }
}

impl RangoRow for MySqlRangoRow {
    fn get_bool(&self, col: &str) -> Result<bool, RowError> {
        MySqlRowRef(&self.0).get_bool(col)
    }
    fn get_i16(&self, col: &str) -> Result<i16, RowError> {
        MySqlRowRef(&self.0).get_i16(col)
    }
    fn get_i32(&self, col: &str) -> Result<i32, RowError> {
        MySqlRowRef(&self.0).get_i32(col)
    }
    fn get_i64(&self, col: &str) -> Result<i64, RowError> {
        MySqlRowRef(&self.0).get_i64(col)
    }
    fn get_f32(&self, col: &str) -> Result<f32, RowError> {
        MySqlRowRef(&self.0).get_f32(col)
    }
    fn get_f64(&self, col: &str) -> Result<f64, RowError> {
        MySqlRowRef(&self.0).get_f64(col)
    }
    fn get_decimal(&self, col: &str) -> Result<rango_orm_core::Decimal, RowError> {
        MySqlRowRef(&self.0).get_decimal(col)
    }
    fn get_string(&self, col: &str) -> Result<String, RowError> {
        MySqlRowRef(&self.0).get_string(col)
    }
    fn get_bytes(&self, col: &str) -> Result<Vec<u8>, RowError> {
        MySqlRowRef(&self.0).get_bytes(col)
    }
    fn get_uuid(&self, col: &str) -> Result<uuid::Uuid, RowError> {
        MySqlRowRef(&self.0).get_uuid(col)
    }
    fn get_datetime(&self, col: &str) -> Result<chrono::DateTime<chrono::Utc>, RowError> {
        MySqlRowRef(&self.0).get_datetime(col)
    }
    fn get_date(&self, col: &str) -> Result<chrono::NaiveDate, RowError> {
        MySqlRowRef(&self.0).get_date(col)
    }
    fn get_time(&self, col: &str) -> Result<chrono::NaiveTime, RowError> {
        MySqlRowRef(&self.0).get_time(col)
    }
    fn get_json(&self, col: &str) -> Result<serde_json::Value, RowError> {
        MySqlRowRef(&self.0).get_json(col)
    }
    fn is_null(&self, col: &str) -> bool {
        MySqlRowRef(&self.0).is_null(col)
    }
}

/// View of a JOIN row restricted to columns aliased `{prefix}{col}`
/// (a `select_related` model), so `R::from_row` can read them by their own names.
pub struct MySqlPrefixedRow<'a> {
    row: &'a MySqlRow,
    prefix: String,
}

impl<'a> MySqlPrefixedRow<'a> {
    pub fn new(row: &'a MySqlRow, prefix: String) -> Self {
        Self { row, prefix }
    }

    fn col(&self, col: &str) -> String {
        format!("{}{}", self.prefix, col)
    }
}

impl RangoRow for MySqlPrefixedRow<'_> {
    fn get_bool(&self, col: &str) -> Result<bool, RowError> {
        MySqlRowRef(self.row).get_bool(&self.col(col))
    }
    fn get_i16(&self, col: &str) -> Result<i16, RowError> {
        MySqlRowRef(self.row).get_i16(&self.col(col))
    }
    fn get_i32(&self, col: &str) -> Result<i32, RowError> {
        MySqlRowRef(self.row).get_i32(&self.col(col))
    }
    fn get_i64(&self, col: &str) -> Result<i64, RowError> {
        MySqlRowRef(self.row).get_i64(&self.col(col))
    }
    fn get_f32(&self, col: &str) -> Result<f32, RowError> {
        MySqlRowRef(self.row).get_f32(&self.col(col))
    }
    fn get_f64(&self, col: &str) -> Result<f64, RowError> {
        MySqlRowRef(self.row).get_f64(&self.col(col))
    }
    fn get_decimal(&self, col: &str) -> Result<rango_orm_core::Decimal, RowError> {
        MySqlRowRef(self.row).get_decimal(&self.col(col))
    }
    fn get_string(&self, col: &str) -> Result<String, RowError> {
        MySqlRowRef(self.row).get_string(&self.col(col))
    }
    fn get_bytes(&self, col: &str) -> Result<Vec<u8>, RowError> {
        MySqlRowRef(self.row).get_bytes(&self.col(col))
    }
    fn get_uuid(&self, col: &str) -> Result<uuid::Uuid, RowError> {
        MySqlRowRef(self.row).get_uuid(&self.col(col))
    }
    fn get_datetime(&self, col: &str) -> Result<chrono::DateTime<chrono::Utc>, RowError> {
        MySqlRowRef(self.row).get_datetime(&self.col(col))
    }
    fn get_date(&self, col: &str) -> Result<chrono::NaiveDate, RowError> {
        MySqlRowRef(self.row).get_date(&self.col(col))
    }
    fn get_time(&self, col: &str) -> Result<chrono::NaiveTime, RowError> {
        MySqlRowRef(self.row).get_time(&self.col(col))
    }
    fn get_json(&self, col: &str) -> Result<serde_json::Value, RowError> {
        MySqlRowRef(self.row).get_json(&self.col(col))
    }
    fn is_null(&self, col: &str) -> bool {
        MySqlRowRef(self.row).is_null(&self.col(col))
    }
}

/// Decode every row into `M`, allocating the result once.
/// (Collecting an iterator of `Result` loses the size hint and regrows the Vec.)
pub(crate) fn decode_all<M: FromRow>(rows: Vec<MySqlRow>) -> rango_orm_core::Result<Vec<M>> {
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(M::from_row(&MySqlRangoRow(r))?);
    }
    Ok(out)
}
