use rango_orm_core::{FromRow, RangoRow, RowError};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, ValueRef};

/// Owned row — used for simple queries.
pub struct SqliteRangoRow(pub SqliteRow);

/// Borrowed row — used for JOIN queries where we need to read the same row twice.
pub struct SqliteRowRef<'a>(pub &'a SqliteRow);

impl RangoRow for SqliteRowRef<'_> {
    fn get_bool(&self, col: &str) -> Result<bool, RowError> {
        self.0
            .try_get::<i64, _>(col)
            .map(|v| v != 0)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_i16(&self, col: &str) -> Result<i16, RowError> {
        self.0
            .try_get::<i64, _>(col)
            .map(|v| v as i16)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_i32(&self, col: &str) -> Result<i32, RowError> {
        self.0
            .try_get::<i64, _>(col)
            .map(|v| v as i32)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_i64(&self, col: &str) -> Result<i64, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_f32(&self, col: &str) -> Result<f32, RowError> {
        self.0
            .try_get::<f64, _>(col)
            .map(|v| v as f32)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_f64(&self, col: &str) -> Result<f64, RowError> {
        self.0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_decimal(&self, col: &str) -> Result<rango_orm_core::Decimal, RowError> {
        // NUMERIC affinity keeps a value as INTEGER or REAL when that is lossless and as
        // TEXT otherwise (more than 15 digits), so the text is the exact one.
        let err = |e: &dyn std::fmt::Display| RowError::new(format!("{}: {}", col, e));
        if let Ok(s) = self.0.try_get::<String, _>(col) {
            return s.parse().map_err(|e| err(&e));
        }
        if let Ok(n) = self.0.try_get::<i64, _>(col) {
            return Ok(n.into());
        }
        let f: f64 = self.0.try_get(col).map_err(|e| err(&e))?;
        rango_orm_core::Decimal::try_from(f).map_err(|e| err(&e))
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
        let s: String = self
            .0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))?;
        s.parse::<uuid::Uuid>()
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_datetime(&self, col: &str) -> Result<chrono::DateTime<chrono::Utc>, RowError> {
        let s: String = self
            .0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))?;
        s.parse::<chrono::DateTime<chrono::Utc>>()
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_date(&self, col: &str) -> Result<chrono::NaiveDate, RowError> {
        let s: String = self
            .0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))?;
        s.parse::<chrono::NaiveDate>()
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_time(&self, col: &str) -> Result<chrono::NaiveTime, RowError> {
        let s: String = self
            .0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))?;
        s.parse::<chrono::NaiveTime>()
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn get_json(&self, col: &str) -> Result<serde_json::Value, RowError> {
        let s: String = self
            .0
            .try_get(col)
            .map_err(|e| RowError::new(format!("{}: {}", col, e)))?;
        serde_json::from_str(&s).map_err(|e| RowError::new(format!("{}: {}", col, e)))
    }
    fn is_null(&self, col: &str) -> bool {
        match self.0.try_get_raw(col) {
            Ok(value) => value.is_null(),
            Err(_) => false,
        }
    }
}

impl RangoRow for SqliteRangoRow {
    fn get_bool(&self, col: &str) -> Result<bool, RowError> {
        SqliteRowRef(&self.0).get_bool(col)
    }
    fn get_i16(&self, col: &str) -> Result<i16, RowError> {
        SqliteRowRef(&self.0).get_i16(col)
    }
    fn get_i32(&self, col: &str) -> Result<i32, RowError> {
        SqliteRowRef(&self.0).get_i32(col)
    }
    fn get_i64(&self, col: &str) -> Result<i64, RowError> {
        SqliteRowRef(&self.0).get_i64(col)
    }
    fn get_f32(&self, col: &str) -> Result<f32, RowError> {
        SqliteRowRef(&self.0).get_f32(col)
    }
    fn get_f64(&self, col: &str) -> Result<f64, RowError> {
        SqliteRowRef(&self.0).get_f64(col)
    }
    fn get_decimal(&self, col: &str) -> Result<rango_orm_core::Decimal, RowError> {
        SqliteRowRef(&self.0).get_decimal(col)
    }
    fn get_string(&self, col: &str) -> Result<String, RowError> {
        SqliteRowRef(&self.0).get_string(col)
    }
    fn get_bytes(&self, col: &str) -> Result<Vec<u8>, RowError> {
        SqliteRowRef(&self.0).get_bytes(col)
    }
    fn get_uuid(&self, col: &str) -> Result<uuid::Uuid, RowError> {
        SqliteRowRef(&self.0).get_uuid(col)
    }
    fn get_datetime(&self, col: &str) -> Result<chrono::DateTime<chrono::Utc>, RowError> {
        SqliteRowRef(&self.0).get_datetime(col)
    }
    fn get_date(&self, col: &str) -> Result<chrono::NaiveDate, RowError> {
        SqliteRowRef(&self.0).get_date(col)
    }
    fn get_time(&self, col: &str) -> Result<chrono::NaiveTime, RowError> {
        SqliteRowRef(&self.0).get_time(col)
    }
    fn get_json(&self, col: &str) -> Result<serde_json::Value, RowError> {
        SqliteRowRef(&self.0).get_json(col)
    }
    fn is_null(&self, col: &str) -> bool {
        SqliteRowRef(&self.0).is_null(col)
    }
}

/// View of a JOIN row restricted to columns aliased `{prefix}{col}`
/// (a `select_related` model), so `R::from_row` can read them by their own names.
pub struct SqlitePrefixedRow<'a> {
    row: &'a SqliteRow,
    prefix: String,
}

impl<'a> SqlitePrefixedRow<'a> {
    pub fn new(row: &'a SqliteRow, prefix: String) -> Self {
        Self { row, prefix }
    }

    fn col(&self, col: &str) -> String {
        format!("{}{}", self.prefix, col)
    }
}

impl RangoRow for SqlitePrefixedRow<'_> {
    fn get_bool(&self, col: &str) -> Result<bool, RowError> {
        SqliteRowRef(self.row).get_bool(&self.col(col))
    }
    fn get_i16(&self, col: &str) -> Result<i16, RowError> {
        SqliteRowRef(self.row).get_i16(&self.col(col))
    }
    fn get_i32(&self, col: &str) -> Result<i32, RowError> {
        SqliteRowRef(self.row).get_i32(&self.col(col))
    }
    fn get_i64(&self, col: &str) -> Result<i64, RowError> {
        SqliteRowRef(self.row).get_i64(&self.col(col))
    }
    fn get_f32(&self, col: &str) -> Result<f32, RowError> {
        SqliteRowRef(self.row).get_f32(&self.col(col))
    }
    fn get_f64(&self, col: &str) -> Result<f64, RowError> {
        SqliteRowRef(self.row).get_f64(&self.col(col))
    }
    fn get_decimal(&self, col: &str) -> Result<rango_orm_core::Decimal, RowError> {
        SqliteRowRef(self.row).get_decimal(&self.col(col))
    }
    fn get_string(&self, col: &str) -> Result<String, RowError> {
        SqliteRowRef(self.row).get_string(&self.col(col))
    }
    fn get_bytes(&self, col: &str) -> Result<Vec<u8>, RowError> {
        SqliteRowRef(self.row).get_bytes(&self.col(col))
    }
    fn get_uuid(&self, col: &str) -> Result<uuid::Uuid, RowError> {
        SqliteRowRef(self.row).get_uuid(&self.col(col))
    }
    fn get_datetime(&self, col: &str) -> Result<chrono::DateTime<chrono::Utc>, RowError> {
        SqliteRowRef(self.row).get_datetime(&self.col(col))
    }
    fn get_date(&self, col: &str) -> Result<chrono::NaiveDate, RowError> {
        SqliteRowRef(self.row).get_date(&self.col(col))
    }
    fn get_time(&self, col: &str) -> Result<chrono::NaiveTime, RowError> {
        SqliteRowRef(self.row).get_time(&self.col(col))
    }
    fn get_json(&self, col: &str) -> Result<serde_json::Value, RowError> {
        SqliteRowRef(self.row).get_json(&self.col(col))
    }
    fn is_null(&self, col: &str) -> bool {
        SqliteRowRef(self.row).is_null(&self.col(col))
    }
}

/// Decode every row into `M`, allocating the result once.
/// (Collecting an iterator of `Result` loses the size hint and regrows the Vec.)
pub(crate) fn decode_all<M: FromRow>(rows: Vec<SqliteRow>) -> rango_orm_core::Result<Vec<M>> {
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(M::from_row(&SqliteRangoRow(r))?);
    }
    Ok(out)
}
