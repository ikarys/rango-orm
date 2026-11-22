/// Error when reading a row.
#[derive(Debug)]
pub struct RowError(String);

impl RowError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl std::fmt::Display for RowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for RowError {}

/// The typed getters of `RangoRow`, `by_name / by_position: type`, fed to a callback macro so
/// the trait defaults and the `&R` forwarding impl stay in sync.
macro_rules! getters {
    ($m:ident) => {
        $m! {
            get_bool / get_bool_at: bool;
            get_i16 / get_i16_at: i16;
            get_i32 / get_i32_at: i32;
            get_i64 / get_i64_at: i64;
            get_f32 / get_f32_at: f32;
            get_f64 / get_f64_at: f64;
            get_decimal / get_decimal_at: rust_decimal::Decimal;
            get_string / get_string_at: String;
            get_bytes / get_bytes_at: Vec<u8>;
            get_uuid / get_uuid_at: uuid::Uuid;
            get_datetime / get_datetime_at: time::OffsetDateTime;
            get_date / get_date_at: time::Date;
            get_time / get_time_at: time::Time;
            get_json / get_json_at: serde_json::Value;
        }
    };
}

macro_rules! trait_methods {
    ($($by:ident / $at:ident: $t:ty;)*) => {
        $(
            fn $by(&self, col: &str) -> Result<$t, RowError>;
            /// `col` is expected at `pos`; a backend may check that and skip the name lookup.
            /// The default ignores `pos`.
            fn $at(&self, _pos: usize, col: &str) -> Result<$t, RowError> {
                self.$by(col)
            }
        )*
    };
}

macro_rules! forward_ref {
    ($($by:ident / $at:ident: $t:ty;)*) => {
        impl<R: RangoRow + ?Sized> RangoRow for &R {
            $(
                #[inline]
                fn $by(&self, col: &str) -> Result<$t, RowError> {
                    (**self).$by(col)
                }
                #[inline]
                fn $at(&self, pos: usize, col: &str) -> Result<$t, RowError> {
                    (**self).$at(pos, col)
                }
            )*
            #[inline]
            fn get_vector(&self, col: &str) -> Result<Vec<f32>, RowError> {
                (**self).get_vector(col)
            }
            #[inline]
            fn get_vector_at(&self, pos: usize, col: &str) -> Result<Vec<f32>, RowError> {
                (**self).get_vector_at(pos, col)
            }
            #[inline]
            fn is_null(&self, col: &str) -> bool {
                (**self).is_null(col)
            }
            #[inline]
            fn is_null_at(&self, pos: usize, col: &str) -> bool {
                (**self).is_null_at(pos, col)
            }
        }
    };
}

/// Backend-agnostic row abstraction.
/// Implemented by rango-orm-postgres (and future backends) for their native row type.
///
/// The `*_at` variants take the position the derive expects the column at (its index in
/// the struct, which is its index in `SELECT *` for a table rango created). A backend
/// may read by position when the name matches, and must fall back to the name otherwise.
pub trait RangoRow {
    getters!(trait_methods);
    /// pgvector column. Only the PostgreSQL backend reads it; the default is an error.
    fn get_vector(&self, col: &str) -> Result<Vec<f32>, RowError> {
        Err(RowError::new(format!(
            "column '{col}': vector columns need the PostgreSQL backend"
        )))
    }
    fn get_vector_at(&self, _pos: usize, col: &str) -> Result<Vec<f32>, RowError> {
        self.get_vector(col)
    }
    fn is_null(&self, col: &str) -> bool;
    fn is_null_at(&self, _pos: usize, col: &str) -> bool {
        self.is_null(col)
    }
}

getters!(forward_ref);

/// Implemented by `#[derive(Model)]` — reads a model from a RangoRow.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be read from a database row",
    note = "add `#[derive(Model)]` (or `#[rango::model]`, or `#[derive(Partial)]` for a column subset) to `{Self}`"
)]
pub trait FromRow: Sized {
    fn from_row(row: &dyn RangoRow) -> Result<Self, RowError>;

    /// `from_row` with the row type known at compile time, so the column reads are static
    /// calls the backend can inline. The derives implement it; the default goes through `dyn`.
    fn from_row_in<R: RangoRow + ?Sized>(row: &R) -> Result<Self, RowError> {
        Self::from_row(&row)
    }
}

/// Implemented by `#[derive(Partial)]` — a subset of a model's columns, read with
/// `QueryBuilder::select_as::<P>()`.
pub trait Partial: FromRow {
    type Model: crate::Model;
    /// Column names to select, taken from the model's typed column constants.
    fn columns() -> &'static [&'static str];
}
