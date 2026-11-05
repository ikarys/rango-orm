use rango_core::SqlValue;
use sqlx::Postgres;
use sqlx::postgres::PgArguments;
use sqlx::query::Query;

/// Default maximum rows returned by [`QueryBuilder::all()`].
/// Override with `.limit(n)` or `.unlimited()`.
pub const DEFAULT_QUERY_LIMIT: i64 = 1000;

// ─── Bind macro ───────────────────────────────────────────────────────────────
macro_rules! bind {
    ($q:expr, $val:expr) => {
        match $val {
            // Typed nulls — bind with the correct Rust type so Postgres infers the column type
            SqlValue::NullBool => $q.bind(Option::<bool>::None),
            SqlValue::NullSmallInt => $q.bind(Option::<i16>::None),
            SqlValue::NullInt => $q.bind(Option::<i32>::None),
            SqlValue::NullBigInt => $q.bind(Option::<i64>::None),
            SqlValue::NullFloat => $q.bind(Option::<f32>::None),
            SqlValue::NullDouble => $q.bind(Option::<f64>::None),
            SqlValue::NullText => $q.bind(Option::<String>::None),
            SqlValue::NullBytes => $q.bind(Option::<Vec<u8>>::None),
            SqlValue::NullUuid => $q.bind(Option::<uuid::Uuid>::None),
            SqlValue::NullDateTime => $q.bind(Option::<chrono::DateTime<chrono::Utc>>::None),
            SqlValue::NullDate => $q.bind(Option::<chrono::NaiveDate>::None),
            SqlValue::NullTime => $q.bind(Option::<chrono::NaiveTime>::None),
            SqlValue::NullJson => $q.bind(Option::<sqlx::types::Json<serde_json::Value>>::None),
            // Non-null values
            SqlValue::Bool(v) => $q.bind(v),
            SqlValue::SmallInt(v) => $q.bind(v),
            SqlValue::Int(v) => $q.bind(v),
            SqlValue::BigInt(v) => $q.bind(v),
            SqlValue::Float(v) => $q.bind(v),
            SqlValue::Double(v) => $q.bind(v),
            SqlValue::Text(v) => $q.bind(v),
            SqlValue::Bytes(v) => $q.bind(v),
            SqlValue::Uuid(v) => $q.bind(v),
            SqlValue::DateTime(v) => $q.bind(v),
            SqlValue::Date(v) => $q.bind(v),
            SqlValue::Time(v) => $q.bind(v),
            SqlValue::Json(v) => $q.bind(sqlx::types::Json(v)),
        }
    };
}

pub fn bind_sql_values<'q>(
    mut q: Query<'q, Postgres, PgArguments>,
    values: Vec<SqlValue>,
) -> Query<'q, Postgres, PgArguments> {
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
