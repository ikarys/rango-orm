use rango_orm_core::SqlValue;
use sqlx::mysql::MySqlArguments;
use sqlx::query::Query;
use sqlx::MySql;

// ─── Bind macro ───────────────────────────────────────────────────────────────
macro_rules! bind {
    ($q:expr, $val:expr) => {
        match $val {
            // Typed nulls
            SqlValue::NullBool => $q.bind(Option::<bool>::None),
            SqlValue::NullSmallInt => $q.bind(Option::<i16>::None),
            SqlValue::NullInt => $q.bind(Option::<i32>::None),
            SqlValue::NullBigInt => $q.bind(Option::<i64>::None),
            SqlValue::NullFloat => $q.bind(Option::<f32>::None),
            SqlValue::NullDouble => $q.bind(Option::<f64>::None),
            SqlValue::NullText => $q.bind(Option::<String>::None),
            SqlValue::NullBytes => $q.bind(Option::<Vec<u8>>::None),
            SqlValue::NullUuid => $q.bind(Option::<Vec<u8>>::None),
            SqlValue::NullDateTime => $q.bind(Option::<chrono::DateTime<chrono::Utc>>::None),
            SqlValue::NullDate => $q.bind(Option::<chrono::NaiveDate>::None),
            SqlValue::NullTime => $q.bind(Option::<chrono::NaiveTime>::None),
            SqlValue::NullJson => $q.bind(Option::<serde_json::Value>::None),
            // Values
            SqlValue::Bool(v) => $q.bind(v),
            SqlValue::SmallInt(v) => $q.bind(v),
            SqlValue::Int(v) => $q.bind(v),
            SqlValue::BigInt(v) => $q.bind(v),
            SqlValue::Float(v) => $q.bind(v),
            SqlValue::Double(v) => $q.bind(v),
            SqlValue::Text(v) => $q.bind(v),
            SqlValue::Bytes(v) => $q.bind(v),
            // UUIDs live in BINARY(16), big-endian bytes (same order as UUID_TO_BIN(x, 0))
            SqlValue::Uuid(v) => $q.bind(v.as_bytes().to_vec()),
            SqlValue::DateTime(v) => $q.bind(v),
            SqlValue::Date(v) => $q.bind(v),
            SqlValue::Time(v) => $q.bind(v),
            SqlValue::Json(v) => $q.bind(v),
        }
    };
}

pub fn bind_sql_values<'q>(
    mut q: Query<'q, MySql, MySqlArguments>,
    values: Vec<SqlValue>,
) -> Query<'q, MySql, MySqlArguments> {
    for val in values {
        q = bind!(q, val);
    }
    q
}

mod bulk;
mod crud;
mod raw;

pub use bulk::*;
pub use crud::*;
pub use raw::*;
