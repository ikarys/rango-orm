use rango_core::SqlValue;
use sqlx::query::Query;
use sqlx::sqlite::SqliteArguments;
use sqlx::Sqlite;

// ─── Bind macro ───────────────────────────────────────────────────────────────
macro_rules! bind {
    ($q:expr, $val:expr) => {
        match $val {
            // Typed nulls
            SqlValue::NullBool => $q.bind(Option::<i64>::None),
            SqlValue::NullSmallInt => $q.bind(Option::<i64>::None),
            SqlValue::NullInt => $q.bind(Option::<i64>::None),
            SqlValue::NullBigInt => $q.bind(Option::<i64>::None),
            SqlValue::NullFloat => $q.bind(Option::<f64>::None),
            SqlValue::NullDouble => $q.bind(Option::<f64>::None),
            SqlValue::NullText => $q.bind(Option::<String>::None),
            SqlValue::NullBytes => $q.bind(Option::<Vec<u8>>::None),
            SqlValue::NullUuid => $q.bind(Option::<String>::None),
            SqlValue::NullDateTime => $q.bind(Option::<String>::None),
            SqlValue::NullDate => $q.bind(Option::<String>::None),
            SqlValue::NullTime => $q.bind(Option::<String>::None),
            SqlValue::NullJson => $q.bind(Option::<String>::None),
            // Values
            SqlValue::Bool(v) => $q.bind(v as i64),
            SqlValue::SmallInt(v) => $q.bind(v as i64),
            SqlValue::Int(v) => $q.bind(v as i64),
            SqlValue::BigInt(v) => $q.bind(v),
            SqlValue::Float(v) => $q.bind(v as f64),
            SqlValue::Double(v) => $q.bind(v),
            SqlValue::Text(v) => $q.bind(v),
            SqlValue::Bytes(v) => $q.bind(v),
            SqlValue::Uuid(v) => $q.bind(v.to_string()),
            SqlValue::DateTime(v) => $q.bind(v.to_rfc3339()),
            SqlValue::Date(v) => $q.bind(v.to_string()),
            SqlValue::Time(v) => $q.bind(v.to_string()),
            SqlValue::Json(v) => $q.bind(v.to_string()),
        }
    };
}

pub fn bind_sql_values<'q>(
    mut q: Query<'q, Sqlite, SqliteArguments<'q>>,
    values: Vec<SqlValue>,
) -> Query<'q, Sqlite, SqliteArguments<'q>> {
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
