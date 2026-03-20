use std::fmt;

/// Marker trait for all Rango field types.
/// Implemented by all Field* types — used by rango-derive to introspect columns.
pub trait RangoField {
    /// The Rust type stored inside this field.
    type Inner;

    /// The SQL column type this field maps to.
    fn column_type() -> crate::schema::ColumnType;

    /// Validate the inner value. Returns an error message if invalid.
    fn validate(value: &Self::Inner) -> Result<(), FieldError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError(pub String);

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ─── Primitive fields ────────────────────────────────────────────────────────

/// Boolean field → BOOLEAN
pub struct FieldBool;

/// Small integer field → SMALLINT
pub struct FieldSmallInt;

/// Integer field → INTEGER
pub struct FieldInt;

/// Big integer field → BIGINT
pub struct FieldBigInt;

/// 32-bit float → REAL
pub struct FieldFloat;

/// 64-bit float → DOUBLE PRECISION
pub struct FieldDouble;

/// Decimal field with precision and scale → NUMERIC(P, S)
pub struct FieldDecimal<const PRECISION: u8, const SCALE: u8>;

// ─── String fields ───────────────────────────────────────────────────────────

/// Unbounded text field → TEXT
pub struct FieldText;

/// Variable-length string with min/max length → VARCHAR(MAX)
/// Validated at construction: value length must be in [MIN, MAX].
pub struct FieldVarchar<const MIN: usize, const MAX: usize>;

/// Email field → VARCHAR(254), validated as email format
pub struct FieldEmail;

/// Password field → VARCHAR(MAX), validated for min/max length
/// Note: Rango stores raw value — hashing is the application's responsibility.
pub struct FieldPassword<const MIN: usize, const MAX: usize>;

/// URL field → VARCHAR(2048), validated as URL format
pub struct FieldUrl;

// ─── Binary ──────────────────────────────────────────────────────────────────

/// Binary field → BYTEA (postgres) / BLOB (mysql/sqlite)
pub struct FieldBytes;

// ─── UUID ─────────────────────────────────────────────────────────────────────

/// UUID field → UUID (postgres) / CHAR(36) (mysql/sqlite)
pub struct FieldUuid;

// ─── Date / Time ─────────────────────────────────────────────────────────────

/// Date only → DATE
pub struct FieldDate;

/// Time only → TIME
pub struct FieldTime;

/// Date + time (with timezone) → TIMESTAMPTZ
pub struct FieldDateTime;

// ─── JSON ────────────────────────────────────────────────────────────────────

/// JSON field → JSONB (postgres) / JSON (others)
pub struct FieldJson;

// ─── Numeric range ───────────────────────────────────────────────────────────

/// Integer with min/max range validation → INTEGER
pub struct FieldRange<const MIN: i64, const MAX: i64>;
