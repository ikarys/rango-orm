use rango_orm_core::row::{FromRow, RangoRow, RowError};
use sqlx::postgres::PgRow;
use sqlx::{Row, ValueRef};

/// Owned row — used for simple queries
pub struct PgRangoRow(pub PgRow);

/// Borrowed row — used for JOIN queries where we need to read the same row twice
pub struct PgRangoRow2<'a>(pub &'a PgRow);

impl<'a> PgRangoRow2<'a> {
    pub fn new(row: &'a PgRow) -> Self {
        Self(row)
    }
}

macro_rules! impl_get {
    ($fn_name:ident, $t:ty) => {
        fn $fn_name(&self, col: &str) -> Result<$t, RowError> {
            self.0
                .try_get::<$t, _>(col)
                .map_err(|e| RowError(format!("Column '{}': {}", col, e)))
        }
    };
}

/// `f64` from FLOAT8, or from NUMERIC (`FieldDecimal` is f64-backed; sqlx has no f64 decoding
/// for NUMERIC without a decimal crate, so the binary wire format is read directly).
fn read_f64(row: &PgRow, col: &str) -> Result<f64, RowError> {
    let err = |e: &dyn std::fmt::Display| RowError(format!("Column '{}': {}", col, e));
    match row.try_get::<f64, _>(col) {
        Ok(v) => Ok(v),
        Err(e) => {
            let raw = row.try_get_raw(col).map_err(|e| err(&e))?;
            let is_numeric = sqlx::TypeInfo::name(&*sqlx::ValueRef::type_info(&raw)) == "NUMERIC";
            let b = raw.as_bytes().map_err(|e| err(&e))?;
            if !is_numeric || b.len() < 8 {
                return Err(err(&e));
            }
            // header: ndigits, weight, sign, dscale (i16/u16 big-endian), then base-10000 digits
            let h = |i: usize| i16::from_be_bytes([b[i], b[i + 1]]);
            let (n, weight, sign) = (h(0) as usize, h(2) as i32, h(4) as u16);
            if sign == 0xC000 {
                return Ok(f64::NAN);
            }
            if b.len() < 8 + 2 * n {
                return Err(err(&"truncated NUMERIC"));
            }
            let mut v = 0f64;
            for i in 0..n {
                v += f64::from(h(8 + 2 * i)) * 10000f64.powi(weight - i as i32);
            }
            Ok(if sign == 0x4000 { -v } else { v })
        }
    }
}

macro_rules! impl_rangorow {
    ($type:ty) => {
        impl RangoRow for $type {
            impl_get!(get_bool, bool);
            impl_get!(get_i16, i16);
            impl_get!(get_i32, i32);
            impl_get!(get_i64, i64);
            impl_get!(get_f32, f32);
            fn get_f64(&self, col: &str) -> Result<f64, RowError> {
                read_f64(&self.0, col)
            }
            impl_get!(get_string, String);
            impl_get!(get_bytes, Vec<u8>);
            impl_get!(get_uuid, uuid::Uuid);
            impl_get!(get_datetime, chrono::DateTime<chrono::Utc>);
            impl_get!(get_date, chrono::NaiveDate);
            impl_get!(get_time, chrono::NaiveTime);

            fn get_json(&self, col: &str) -> Result<serde_json::Value, RowError> {
                self.0
                    .try_get::<sqlx::types::Json<serde_json::Value>, _>(col)
                    .map(|j| j.0)
                    .map_err(|e| RowError(format!("Column '{}': {}", col, e)))
            }

            fn is_null(&self, col: &str) -> bool {
                match self.0.try_get_raw(col) {
                    Ok(value) => value.is_null(),
                    Err(_) => false,
                }
            }
        }
    };
}

impl_rangorow!(PgRangoRow);
impl_rangorow!(PgRangoRow2<'_>);

/// View of a JOIN row restricted to columns aliased `{prefix}{col}`
/// (a `select_related` model), so `R::from_row` can read them by their own names.
pub struct PgPrefixedRow<'a> {
    row: &'a PgRow,
    prefix: String,
}

impl<'a> PgPrefixedRow<'a> {
    pub fn new(row: &'a PgRow, prefix: String) -> Self {
        Self { row, prefix }
    }

    fn col(&self, col: &str) -> String {
        format!("{}{}", self.prefix, col)
    }
}

impl RangoRow for PgPrefixedRow<'_> {
    fn get_bool(&self, col: &str) -> Result<bool, RowError> {
        PgRangoRow2(self.row).get_bool(&self.col(col))
    }
    fn get_i16(&self, col: &str) -> Result<i16, RowError> {
        PgRangoRow2(self.row).get_i16(&self.col(col))
    }
    fn get_i32(&self, col: &str) -> Result<i32, RowError> {
        PgRangoRow2(self.row).get_i32(&self.col(col))
    }
    fn get_i64(&self, col: &str) -> Result<i64, RowError> {
        PgRangoRow2(self.row).get_i64(&self.col(col))
    }
    fn get_f32(&self, col: &str) -> Result<f32, RowError> {
        PgRangoRow2(self.row).get_f32(&self.col(col))
    }
    fn get_f64(&self, col: &str) -> Result<f64, RowError> {
        PgRangoRow2(self.row).get_f64(&self.col(col))
    }
    fn get_string(&self, col: &str) -> Result<String, RowError> {
        PgRangoRow2(self.row).get_string(&self.col(col))
    }
    fn get_bytes(&self, col: &str) -> Result<Vec<u8>, RowError> {
        PgRangoRow2(self.row).get_bytes(&self.col(col))
    }
    fn get_uuid(&self, col: &str) -> Result<uuid::Uuid, RowError> {
        PgRangoRow2(self.row).get_uuid(&self.col(col))
    }
    fn get_datetime(&self, col: &str) -> Result<chrono::DateTime<chrono::Utc>, RowError> {
        PgRangoRow2(self.row).get_datetime(&self.col(col))
    }
    fn get_date(&self, col: &str) -> Result<chrono::NaiveDate, RowError> {
        PgRangoRow2(self.row).get_date(&self.col(col))
    }
    fn get_time(&self, col: &str) -> Result<chrono::NaiveTime, RowError> {
        PgRangoRow2(self.row).get_time(&self.col(col))
    }
    fn get_json(&self, col: &str) -> Result<serde_json::Value, RowError> {
        PgRangoRow2(self.row).get_json(&self.col(col))
    }
    fn is_null(&self, col: &str) -> bool {
        PgRangoRow2(self.row).is_null(&self.col(col))
    }
}

/// Decode every row into `M`, allocating the result once.
/// (Collecting an iterator of `Result` loses the size hint and regrows the Vec.)
pub(crate) fn decode_all<M: FromRow>(rows: Vec<PgRow>) -> rango_orm_core::Result<Vec<M>> {
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(M::from_row(&PgRangoRow(r))?);
    }
    Ok(out)
}
