use rango_orm_core::row::{FromRow, RangoRow, RowError};
use sqlx::postgres::PgRow;
use sqlx::{Column, Row, ValueRef};

/// Owned row — used for simple queries
pub struct PgRangoRow(pub PgRow);

/// Borrowed row — used for JOIN queries where we need to read the same row twice
pub struct PgRangoRow2<'a>(pub &'a PgRow);

impl<'a> PgRangoRow2<'a> {
    pub fn new(row: &'a PgRow) -> Self {
        Self(row)
    }
}

/// `pos` if `col` sits there: one string compare instead of the hashed name lookup.
/// `None` when the row's column order differs from the model (hand-made table, custom
/// `SELECT`), so the caller falls back to the name.
#[inline]
fn at(row: &PgRow, pos: usize, col: &str) -> Option<usize> {
    row.columns()
        .get(pos)
        .filter(|c| c.name() == col)
        .map(|_| pos)
}

fn col_err(col: &str, e: sqlx::Error) -> RowError {
    RowError::new(format!("Column '{}': {}", col, e))
}

// `#[inline]`: the derive calls these from the user's crate; without it, non-LTO builds
// pay a call per column.
macro_rules! impl_get {
    ($by:ident, $at:ident, $t:ty) => {
        #[inline]
        fn $by(&self, col: &str) -> Result<$t, RowError> {
            self.0.try_get::<$t, _>(col).map_err(|e| col_err(col, e))
        }
        #[inline]
        fn $at(&self, pos: usize, col: &str) -> Result<$t, RowError> {
            match at(&self.0, pos, col) {
                Some(i) => self.0.try_get::<$t, _>(i),
                None => self.0.try_get::<$t, _>(col),
            }
            .map_err(|e| col_err(col, e))
        }
    };
}

macro_rules! impl_rangorow {
    ($type:ty) => {
        impl RangoRow for $type {
            impl_get!(get_bool, get_bool_at, bool);
            impl_get!(get_i16, get_i16_at, i16);
            impl_get!(get_i32, get_i32_at, i32);
            impl_get!(get_i64, get_i64_at, i64);
            impl_get!(get_f32, get_f32_at, f32);
            impl_get!(get_f64, get_f64_at, f64);
            impl_get!(get_decimal, get_decimal_at, rango_orm_core::Decimal);
            impl_get!(get_string, get_string_at, String);
            impl_get!(get_bytes, get_bytes_at, Vec<u8>);
            impl_get!(get_uuid, get_uuid_at, uuid::Uuid);
            impl_get!(get_datetime, get_datetime_at, chrono::DateTime<chrono::Utc>);
            impl_get!(get_date, get_date_at, chrono::NaiveDate);
            impl_get!(get_time, get_time_at, chrono::NaiveTime);

            #[inline]
            fn get_json(&self, col: &str) -> Result<serde_json::Value, RowError> {
                self.0
                    .try_get::<sqlx::types::Json<serde_json::Value>, _>(col)
                    .map(|j| j.0)
                    .map_err(|e| col_err(col, e))
            }
            #[inline]
            fn get_json_at(&self, pos: usize, col: &str) -> Result<serde_json::Value, RowError> {
                match at(&self.0, pos, col) {
                    Some(i) => self.0.try_get::<sqlx::types::Json<serde_json::Value>, _>(i),
                    None => self
                        .0
                        .try_get::<sqlx::types::Json<serde_json::Value>, _>(col),
                }
                .map(|j| j.0)
                .map_err(|e| col_err(col, e))
            }

            #[inline]
            fn is_null(&self, col: &str) -> bool {
                self.0
                    .try_get_raw(col)
                    .map(|v| v.is_null())
                    .unwrap_or(false)
            }
            #[inline]
            fn is_null_at(&self, pos: usize, col: &str) -> bool {
                match at(&self.0, pos, col) {
                    Some(i) => self.0.try_get_raw(i),
                    None => self.0.try_get_raw(col),
                }
                .map(|v| v.is_null())
                .unwrap_or(false)
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
    fn get_decimal(&self, col: &str) -> Result<rango_orm_core::Decimal, RowError> {
        PgRangoRow2(self.row).get_decimal(&self.col(col))
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

/// `fetch_all` decoding each row as it arrives, so the query never holds every `PgRow`.
pub(crate) async fn fetch_decoded<'e, M, E>(
    q: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
    executor: E,
) -> rango_orm_core::Result<Vec<M>>
where
    M: FromRow,
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    // A loop over the stream rather than `try_map`: that would bound `M: Send + Unpin`
    // on every public query method.
    let mut rows = q.fetch(executor);
    let mut out = Vec::new();
    while let Some(row) = std::future::poll_fn(|cx| rows.as_mut().poll_next(cx)).await {
        out.push(M::from_row_in(&PgRangoRow(
            row.map_err(crate::error::db_err)?,
        ))?);
    }
    Ok(out)
}
