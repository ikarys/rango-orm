use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Marker trait for all Rango field types.
pub trait RangoField {
    type Inner;
    fn column_type() -> crate::schema::ColumnType;
    fn into_inner(self) -> Self::Inner;
}

// ─── Primitive fields ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldBool(pub bool);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSmallInt(pub i16);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldInt(pub i32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldBigInt(pub i64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldFloat(pub f32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDouble(pub f64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDecimal<const PRECISION: u8, const SCALE: u8>(pub f64);

// ─── String fields ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldText(pub String);

/// VARCHAR with min/max length validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldVarchar<const MIN: usize, const MAX: usize>(pub String);

impl<const MIN: usize, const MAX: usize> FieldVarchar<MIN, MAX> {
    pub fn new(s: impl Into<String>) -> Result<Self, FieldError> {
        let s = s.into();
        if s.len() < MIN {
            return Err(FieldError(format!("Value too short (min {})", MIN)));
        }
        if s.len() > MAX {
            return Err(FieldError(format!("Value too long (max {})", MAX)));
        }
        Ok(Self(s))
    }
}

/// Email field — validated on construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldEmail(pub String);

impl FieldEmail {
    pub fn new(s: impl Into<String>) -> Result<Self, FieldError> {
        let s = s.into();
        if !s.contains('@') || s.len() > 254 {
            return Err(FieldError("Invalid email address".into()));
        }
        Ok(Self(s))
    }
}

/// Password field — length validated on construction. Value is stored as-is.
/// Hashing is the application's responsibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldPassword<const MIN: usize, const MAX: usize>(pub String);

impl<const MIN: usize, const MAX: usize> FieldPassword<MIN, MAX> {
    pub fn new(s: impl Into<String>) -> Result<Self, FieldError> {
        let s = s.into();
        if s.len() < MIN {
            return Err(FieldError(format!("Password too short (min {})", MIN)));
        }
        if s.len() > MAX {
            return Err(FieldError(format!("Password too long (max {})", MAX)));
        }
        Ok(Self(s))
    }
}

/// URL field — basic validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldUrl(pub String);

impl FieldUrl {
    pub fn new(s: impl Into<String>) -> Result<Self, FieldError> {
        let s = s.into();
        if !s.starts_with("http://") && !s.starts_with("https://") {
            return Err(FieldError("URL must start with http:// or https://".into()));
        }
        Ok(Self(s))
    }
}

// ─── Binary ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldBytes(pub Vec<u8>);

// ─── UUID ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldUuid(pub Uuid);

impl FieldUuid {
    /// Generate a new random UUID v4.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for FieldUuid {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Date / Time ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDate(pub NaiveDate);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldTime(pub NaiveTime);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDateTime(pub DateTime<Utc>);

impl FieldDateTime {
    pub fn now() -> Self {
        Self(Utc::now())
    }
}

// ─── JSON ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldJson(pub serde_json::Value);

// ─── Range ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldRange<const MIN: i64, const MAX: i64>(pub i64);

impl<const MIN: i64, const MAX: i64> FieldRange<MIN, MAX> {
    pub fn new(val: i64) -> Result<Self, FieldError> {
        if val < MIN || val > MAX {
            return Err(FieldError(format!(
                "Value {} out of range [{}, {}]",
                val, MIN, MAX
            )));
        }
        Ok(Self(val))
    }
}

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct FieldError(pub String);

impl std::fmt::Display for FieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for FieldError {}

// ─── ToSqlValue implementations ──────────────────────────────────────────────

use crate::query::{SqlValue, ToSqlValue};

impl ToSqlValue for FieldBool {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Bool(self.0)
    }
}
impl ToSqlValue for FieldSmallInt {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::SmallInt(self.0)
    }
}
impl ToSqlValue for FieldInt {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Int(self.0)
    }
}
impl ToSqlValue for FieldBigInt {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::BigInt(self.0)
    }
}
impl ToSqlValue for FieldFloat {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Float(self.0)
    }
}
impl ToSqlValue for FieldDouble {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Double(self.0)
    }
}
impl ToSqlValue for FieldText {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Text(self.0.clone())
    }
}
impl ToSqlValue for FieldEmail {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Text(self.0.clone())
    }
}
impl ToSqlValue for FieldUrl {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Text(self.0.clone())
    }
}
impl ToSqlValue for FieldBytes {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Bytes(self.0.clone())
    }
}
impl ToSqlValue for FieldUuid {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Uuid(self.0)
    }
}
impl ToSqlValue for FieldDateTime {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::DateTime(self.0)
    }
}
impl ToSqlValue for FieldDate {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Date(self.0)
    }
}
impl ToSqlValue for FieldTime {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Time(self.0)
    }
}
impl ToSqlValue for FieldJson {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Json(self.0.clone())
    }
}

impl<const MIN: usize, const MAX: usize> ToSqlValue for FieldVarchar<MIN, MAX> {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Text(self.0.clone())
    }
}
impl<const MIN: usize, const MAX: usize> ToSqlValue for FieldPassword<MIN, MAX> {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Text(self.0.clone())
    }
}
impl<const MIN: i64, const MAX: i64> ToSqlValue for FieldRange<MIN, MAX> {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::BigInt(self.0)
    }
}
impl<const P: u8, const S: u8> ToSqlValue for FieldDecimal<P, S> {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Double(self.0)
    }
}

// Option<T> support — typed nulls so Postgres knows the column type
impl ToSqlValue for Option<FieldBool> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullBool, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldSmallInt> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullSmallInt, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldInt> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullInt, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldBigInt> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullBigInt, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldFloat> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullFloat, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldDouble> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullDouble, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldText> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullText, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldEmail> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullText, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldUrl> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullText, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldBytes> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullBytes, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldUuid> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullUuid, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldDateTime> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullDateTime, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldDate> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullDate, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldTime> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullTime, |v| v.to_sql_value())
    }
}
impl ToSqlValue for Option<FieldJson> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullJson, |v| v.to_sql_value())
    }
}

// Parametric types
impl<const MIN: usize, const MAX: usize> ToSqlValue for Option<FieldVarchar<MIN, MAX>> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullText, |v| v.to_sql_value())
    }
}
impl<const MIN: usize, const MAX: usize> ToSqlValue for Option<FieldPassword<MIN, MAX>> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullText, |v| v.to_sql_value())
    }
}
impl<const MIN: i64, const MAX: i64> ToSqlValue for Option<FieldRange<MIN, MAX>> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullBigInt, |v| v.to_sql_value())
    }
}
impl<M> ToSqlValue for Option<ForeignKey<M>> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullUuid, |v| v.to_sql_value())
    }
}

// ─── Many to Many ────────────────────────────────────────────────────────────

/// A many-to-many relation field.
/// The pivot table is auto-generated by `rango makemigrations`.
///
/// # Example
/// ```rust,ignore
/// #[derive(Model)]
/// struct Article {
///     id: FieldUuid,
///     tags: ManyToMany<Tag>,                      // auto pivot table
///
///     #[field(through = "ArticleTag")]
///     contributors: ManyToMany<User>,             // explicit pivot table
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManyToMany<M>(
    /// Cached related IDs — populated by prefetch_related, empty otherwise.
    pub Vec<uuid::Uuid>,
    std::marker::PhantomData<M>,
);

impl<M> ManyToMany<M> {
    pub fn empty() -> Self {
        Self(Vec::new(), std::marker::PhantomData)
    }
}

// ─── Foreign Key ─────────────────────────────────────────────────────────────

/// A foreign key reference — stores the PK value of the related model.
/// The related model type `M` is used for type safety and select_related.
///
/// # Example
/// ```rust,ignore
/// #[derive(Model)]
/// struct Article {
///     id: FieldUuid,
///     author_id: ForeignKey<User>,
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForeignKey<M>(pub uuid::Uuid, std::marker::PhantomData<M>);

impl<M> ForeignKey<M> {
    pub fn new(id: uuid::Uuid) -> Self {
        Self(id, std::marker::PhantomData)
    }

    pub fn id(&self) -> uuid::Uuid {
        self.0
    }
}

impl<M> From<uuid::Uuid> for ForeignKey<M> {
    fn from(id: uuid::Uuid) -> Self {
        Self::new(id)
    }
}

impl<M> ToSqlValue for ForeignKey<M> {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Uuid(self.0)
    }
}

// ─── From<T> conversions (ergonomics) ────────────────────────────────────────

impl From<bool> for FieldBool {
    fn from(v: bool) -> Self {
        Self(v)
    }
}
impl From<i16> for FieldSmallInt {
    fn from(v: i16) -> Self {
        Self(v)
    }
}
impl From<i32> for FieldInt {
    fn from(v: i32) -> Self {
        Self(v)
    }
}
impl From<i64> for FieldBigInt {
    fn from(v: i64) -> Self {
        Self(v)
    }
}
impl From<f32> for FieldFloat {
    fn from(v: f32) -> Self {
        Self(v)
    }
}
impl From<f64> for FieldDouble {
    fn from(v: f64) -> Self {
        Self(v)
    }
}
impl From<String> for FieldText {
    fn from(v: String) -> Self {
        Self(v)
    }
}
impl From<&str> for FieldText {
    fn from(v: &str) -> Self {
        Self(v.to_string())
    }
}
impl From<Vec<u8>> for FieldBytes {
    fn from(v: Vec<u8>) -> Self {
        Self(v)
    }
}
impl From<Uuid> for FieldUuid {
    fn from(v: Uuid) -> Self {
        Self(v)
    }
}
impl From<NaiveDate> for FieldDate {
    fn from(v: NaiveDate) -> Self {
        Self(v)
    }
}
impl From<NaiveTime> for FieldTime {
    fn from(v: NaiveTime) -> Self {
        Self(v)
    }
}
impl From<DateTime<Utc>> for FieldDateTime {
    fn from(v: DateTime<Utc>) -> Self {
        Self(v)
    }
}
impl From<serde_json::Value> for FieldJson {
    fn from(v: serde_json::Value) -> Self {
        Self(v)
    }
}
