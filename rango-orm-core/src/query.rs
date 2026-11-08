impl SqlValue {
    /// True for every typed NULL variant.
    pub fn is_null(&self) -> bool {
        matches!(
            self,
            SqlValue::NullBool
                | SqlValue::NullSmallInt
                | SqlValue::NullInt
                | SqlValue::NullBigInt
                | SqlValue::NullFloat
                | SqlValue::NullDouble
                | SqlValue::NullDecimal
                | SqlValue::NullText
                | SqlValue::NullBytes
                | SqlValue::NullUuid
                | SqlValue::NullDateTime
                | SqlValue::NullDate
                | SqlValue::NullTime
                | SqlValue::NullJson
        )
    }
}

/// A SQL-compatible value — used to bind field values to queries
/// without requiring sqlx as a direct dependency of rango-orm-core.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    // Typed nulls — carry the column type so the DB can infer correctly.
    NullBool,
    NullSmallInt,
    NullInt,
    NullBigInt,
    NullFloat,
    NullDouble,
    NullDecimal,
    NullText,
    NullBytes,
    NullUuid,
    NullDateTime,
    NullDate,
    NullTime,
    NullJson,
    // Non-null values
    Bool(bool),
    SmallInt(i16),
    Int(i32),
    BigInt(i64),
    Float(f32),
    Double(f64),
    Decimal(rust_decimal::Decimal),
    Text(String),
    Bytes(Vec<u8>),
    Uuid(uuid::Uuid),
    DateTime(chrono::DateTime<chrono::Utc>),
    Date(chrono::NaiveDate),
    Time(chrono::NaiveTime),
    Json(serde_json::Value),
}

/// Implemented by field types — converts to a SQL-bindable value.
pub trait ToSqlValue {
    fn to_sql_value(&self) -> SqlValue;
}

// ─── From<T> conversions for SqlValue ────────────────────────────────────────

impl From<&str> for SqlValue {
    fn from(s: &str) -> Self {
        SqlValue::Text(s.to_string())
    }
}
impl From<String> for SqlValue {
    fn from(s: String) -> Self {
        SqlValue::Text(s)
    }
}
impl From<bool> for SqlValue {
    fn from(v: bool) -> Self {
        SqlValue::Bool(v)
    }
}
impl From<i16> for SqlValue {
    fn from(v: i16) -> Self {
        SqlValue::SmallInt(v)
    }
}
impl From<i32> for SqlValue {
    fn from(v: i32) -> Self {
        SqlValue::Int(v)
    }
}
impl From<i64> for SqlValue {
    fn from(v: i64) -> Self {
        SqlValue::BigInt(v)
    }
}
impl From<f32> for SqlValue {
    fn from(v: f32) -> Self {
        SqlValue::Float(v)
    }
}
impl From<f64> for SqlValue {
    fn from(v: f64) -> Self {
        SqlValue::Double(v)
    }
}
impl From<rust_decimal::Decimal> for SqlValue {
    fn from(v: rust_decimal::Decimal) -> Self {
        SqlValue::Decimal(v)
    }
}
impl From<uuid::Uuid> for SqlValue {
    fn from(v: uuid::Uuid) -> Self {
        SqlValue::Uuid(v)
    }
}
impl From<chrono::DateTime<chrono::Utc>> for SqlValue {
    fn from(v: chrono::DateTime<chrono::Utc>) -> Self {
        SqlValue::DateTime(v)
    }
}
impl From<chrono::NaiveDate> for SqlValue {
    fn from(v: chrono::NaiveDate) -> Self {
        SqlValue::Date(v)
    }
}
impl From<chrono::NaiveTime> for SqlValue {
    fn from(v: chrono::NaiveTime) -> Self {
        SqlValue::Time(v)
    }
}
impl From<Vec<u8>> for SqlValue {
    fn from(v: Vec<u8>) -> Self {
        SqlValue::Bytes(v)
    }
}
impl From<serde_json::Value> for SqlValue {
    fn from(v: serde_json::Value) -> Self {
        SqlValue::Json(v)
    }
}

/// Marker trait — implemented by #[derive(Model)] to enable filter() method.
/// The actual QueryBuilder is in rango-orm-postgres to avoid circular deps.
pub trait Filterable: Sized {}

/// Implemented by `#[derive(Model)]` — provides field names + values for INSERT/UPDATE.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be written to the database",
    note = "add `#[derive(Model)]` (or `#[rango::model]`) to `{Self}`"
)]
pub trait ModelValues {
    /// Returns (column_name, value) pairs for all non-PK fields.
    fn field_values(&self) -> Vec<(&'static str, SqlValue)>;

    /// Returns the primary key value.
    fn pk_value(&self) -> SqlValue;

    /// Returns the primary key column name.
    fn pk_column() -> &'static str;

    /// Stores a database-generated primary key (auto-increment PKs only; no-op otherwise).
    /// Used by backends without `RETURNING` (MySQL).
    fn set_generated_pk(&mut self, _id: i64) {}
}

/// Columns and values to INSERT for `model`. A NULL primary key (an unsaved
/// `FieldAutoInt` / `FieldAutoBigInt`) is left out so the database generates it.
pub fn insert_values<M: ModelValues>(model: &M) -> Vec<(&'static str, SqlValue)> {
    let pk = model.pk_value();
    let fields = model.field_values();
    let mut values = Vec::with_capacity(fields.len() + 1);
    if !pk.is_null() {
        values.push((M::pk_column(), pk));
    }
    values.extend(fields);
    values
}

/// Quote a SQL identifier (table or column name), doubling embedded `"`.
/// Valid for PostgreSQL and SQLite.
pub fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Quote a MySQL / MariaDB identifier with backticks, doubling embedded `` ` ``.
pub fn quote_ident_mysql(ident: &str) -> String {
    format!("`{}`", ident.replace('`', "``"))
}

#[cfg(test)]
mod quote_ident_tests {
    use super::{quote_ident, quote_ident_mysql};

    #[test]
    fn test_quote_ident_mysql_escapes_backticks() {
        assert_eq!(quote_ident_mysql("user"), "`user`");
        assert_eq!(
            quote_ident_mysql("a`; DROP TABLE x; --"),
            "`a``; DROP TABLE x; --`"
        );
    }

    #[test]
    fn test_quote_ident_escapes_double_quotes() {
        assert_eq!(quote_ident("name"), "\"name\"");
        assert_eq!(
            quote_ident("a\"; DROP TABLE x; --"),
            "\"a\"\"; DROP TABLE x; --\""
        );
    }
}
