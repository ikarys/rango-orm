use argon2::Argon2;
use argon2::password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier};
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

/// Auto-increment `INTEGER` primary key. `None` until the row is inserted;
/// the database generates the value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct FieldAutoInt(pub Option<i32>);

/// Auto-increment `BIGINT` primary key. `None` until the row is inserted;
/// the database generates the value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct FieldAutoBigInt(pub Option<i64>);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldFloat(pub f32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDouble(pub f64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDecimal<const PRECISION: u8, const SCALE: u8>(pub rust_decimal::Decimal);

impl<const P: u8, const S: u8> FieldDecimal<P, S> {
    /// Wraps `value` at the column scale (`12.5` becomes `12.50` for `FieldDecimal<10, 2>`),
    /// so a value read back has the same digits the database stores.
    pub fn from_decimal(mut value: rust_decimal::Decimal) -> Self {
        value.rescale(u32::from(S));
        Self(value)
    }
}

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

/// Password field — stores an Argon2id PHC hash, never the plaintext.
///
/// The plaintext is length-validated, then hashed with a random salt.
/// `Debug` prints a redacted value. This type intentionally does not implement
/// `Serialize`/`Deserialize`; if a model must be serialized, skip the field
/// or use a DTO:
///
/// ```rust,ignore
/// #[derive(serde::Serialize)]
/// struct UserDto {
///     #[serde(skip)]
///     password: FieldPassword<8, 128>,
/// }
/// ```
#[derive(Clone, PartialEq)]
pub struct FieldPassword<const MIN: usize, const MAX: usize> {
    hash: String,
}

impl<const MIN: usize, const MAX: usize> std::fmt::Debug for FieldPassword<MIN, MAX> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FieldPassword(<redacted>)")
    }
}

impl<const MIN: usize, const MAX: usize> FieldPassword<MIN, MAX> {
    pub fn new(plain: impl AsRef<str>) -> Result<Self, FieldError> {
        let plain = plain.as_ref();
        let len = plain.chars().count();
        if len < MIN {
            return Err(FieldError(format!("Password too short (min {})", MIN)));
        }
        if len > MAX {
            return Err(FieldError(format!("Password too long (max {})", MAX)));
        }

        let hash = Argon2::default()
            .hash_password(plain.as_bytes())
            .map_err(|e| FieldError(e.to_string()))?;

        Ok(Self {
            hash: hash.to_string(),
        })
    }

    pub fn verify(&self, candidate: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(&self.hash) else {
            return false;
        };

        Argon2::default()
            .verify_password(candidate.as_bytes(), &parsed)
            .is_ok()
    }

    pub fn from_hash(hash: String) -> Self {
        Self { hash }
    }

    pub fn hash(&self) -> &str {
        &self.hash
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
impl ToSqlValue for FieldAutoInt {
    fn to_sql_value(&self) -> SqlValue {
        self.0.map_or(SqlValue::NullInt, SqlValue::Int)
    }
}
impl ToSqlValue for FieldAutoBigInt {
    fn to_sql_value(&self) -> SqlValue {
        self.0.map_or(SqlValue::NullBigInt, SqlValue::BigInt)
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
        SqlValue::Text(self.hash().to_string())
    }
}
impl<const MIN: i64, const MAX: i64> ToSqlValue for FieldRange<MIN, MAX> {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::BigInt(self.0)
    }
}
impl<const P: u8, const S: u8> ToSqlValue for FieldDecimal<P, S> {
    fn to_sql_value(&self) -> SqlValue {
        SqlValue::Decimal(self.0)
    }
}
impl<const P: u8, const S: u8> ToSqlValue for Option<FieldDecimal<P, S>> {
    fn to_sql_value(&self) -> SqlValue {
        self.as_ref()
            .map_or(SqlValue::NullDecimal, |v| v.to_sql_value())
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
    /// Always empty when loaded from the database: use `.prefetch_related(Model::FIELD)`
    /// and `.prefetched::<M>()` to load the related rows.
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
impl From<i32> for FieldAutoInt {
    fn from(v: i32) -> Self {
        Self(Some(v))
    }
}
impl From<i64> for FieldAutoBigInt {
    fn from(v: i64) -> Self {
        Self(Some(v))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_password_new_rejects_too_short() {
        assert!(FieldPassword::<8, 128>::new("short").is_err());
    }

    #[test]
    fn field_password_new_rejects_too_long() {
        let long = "a".repeat(129);
        assert!(FieldPassword::<8, 128>::new(long).is_err());
    }

    #[test]
    fn field_password_verify_accepts_correct_and_rejects_wrong() {
        let password = FieldPassword::<8, 128>::new("correct horse battery staple").unwrap();
        assert!(password.verify("correct horse battery staple"));
        assert!(!password.verify("wrong password"));
    }

    #[test]
    fn field_password_hash_is_argon2id() {
        let password = FieldPassword::<8, 128>::new("secret password").unwrap();
        assert!(password.hash().starts_with("$argon2id$"));
    }

    #[test]
    fn field_password_debug_does_not_leak_plaintext() {
        let password = FieldPassword::<8, 128>::new("super secret").unwrap();
        let debug = format!("{:?}", password);
        assert!(!debug.contains("super secret"));
        assert!(debug.contains("FieldPassword(<redacted>)"));
    }
}

// ─── FilterValue: plain Rust values accepted by typed column filters ────────

use crate::column::FilterValue;

macro_rules! filter_values {
    ($($field:ty => $($value:ty),+;)+) => {$($(
        impl FilterValue<$field> for $value {
            fn into_sql_value(self) -> SqlValue {
                self.into()
            }
        }
    )+)+};
}

filter_values! {
    FieldBool => bool;
    FieldSmallInt => i16;
    FieldInt => i32;
    FieldBigInt => i64;
    FieldAutoInt => i32;
    FieldAutoBigInt => i64;
    FieldFloat => f32;
    FieldDouble => f64;
    FieldText => String, &str;
    FieldEmail => String, &str;
    FieldUrl => String, &str;
    FieldBytes => Vec<u8>;
    FieldUuid => Uuid;
    FieldDateTime => DateTime<Utc>;
    FieldDate => NaiveDate;
    FieldTime => NaiveTime;
    FieldJson => serde_json::Value;
}

impl<const MIN: usize, const MAX: usize> FilterValue<FieldVarchar<MIN, MAX>> for String {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Text(self)
    }
}
impl<const MIN: usize, const MAX: usize> FilterValue<FieldVarchar<MIN, MAX>> for &str {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Text(self.to_string())
    }
}
impl<const MIN: i64, const MAX: i64> FilterValue<FieldRange<MIN, MAX>> for i64 {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::BigInt(self)
    }
}
impl<const P: u8, const S: u8> FilterValue<FieldDecimal<P, S>> for rust_decimal::Decimal {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Decimal(self)
    }
}
impl<M> FilterValue<ForeignKey<M>> for Uuid {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Uuid(self)
    }
}
impl<M> FilterValue<ForeignKey<M>> for FieldUuid {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Uuid(self.0)
    }
}
