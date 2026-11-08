//! Typed errors returned by every Rango operation.
//!
//! Backends translate driver errors into these variants so applications can react
//! (`UniqueViolation` → HTTP 409, `Conflict` → retry) without string matching.
//! Messages never contain SQL text or bound values.

use std::fmt;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The query expected a row and found none.
    NotFound,
    /// The table does not exist: migrations were not generated or applied.
    TableMissing {
        table: String,
    },
    UniqueViolation {
        constraint: Option<String>,
    },
    ForeignKeyViolation {
        constraint: Option<String>,
    },
    NotNullViolation {
        constraint: Option<String>,
    },
    CheckViolation {
        constraint: Option<String>,
    },
    /// Serialization failure or deadlock: retrying the whole transaction may succeed.
    Conflict,
    /// Pool acquire timeout, statement timeout or lock timeout.
    Timeout,
    /// Connection could not be opened or was lost (IO, TLS, closed pool).
    Connection(String),
    /// A row could not be decoded into the model.
    Decode(String),
    /// Invalid input or configuration, detected before or by the database.
    Invalid(String),
    /// Any other database error. `code` is the backend code (SQLSTATE on Postgres).
    Database {
        code: Option<String>,
        message: String,
    },
    /// User code aborted the operation (e.g. inside `atomic`), which rolls it back.
    Aborted(Box<dyn std::error::Error + Send + Sync>),
    /// A `ModelHooks` hook returned an error.
    Hook(Box<dyn std::error::Error + Send + Sync>),
}

impl Error {
    /// Abort a transaction with an application error: `return Err(Error::abort("insufficient funds"))`.
    pub fn abort(err: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Error::Aborted(err.into())
    }

    /// True for errors where retrying the transaction can succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Error::Conflict)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let constraint =
            |c: &Option<String>| c.as_deref().map(|c| format!(" ({c})")).unwrap_or_default();
        match self {
            Error::NotFound => write!(f, "row not found"),
            Error::TableMissing { table } => write!(
                f,
                "table `{table}` does not exist — run `rango makemigrations` and `rango migrate`"
            ),
            Error::UniqueViolation { constraint: c } => {
                write!(f, "unique violation{}", constraint(c))
            }
            Error::ForeignKeyViolation { constraint: c } => {
                write!(f, "foreign key violation{}", constraint(c))
            }
            Error::NotNullViolation { constraint: c } => {
                write!(f, "not null violation{}", constraint(c))
            }
            Error::CheckViolation { constraint: c } => {
                write!(f, "check violation{}", constraint(c))
            }
            Error::Conflict => write!(
                f,
                "transaction conflict (serialization failure or deadlock)"
            ),
            Error::Timeout => write!(f, "timed out"),
            Error::Connection(m) => write!(f, "connection error: {m}"),
            Error::Decode(m) => write!(f, "decode error: {m}"),
            Error::Invalid(m) => write!(f, "invalid: {m}"),
            Error::Database {
                code: Some(c),
                message,
            } => write!(f, "database error {c}: {message}"),
            Error::Database {
                code: None,
                message,
            } => write!(f, "database error: {message}"),
            Error::Aborted(e) => write!(f, "aborted: {e}"),
            Error::Hook(e) => write!(f, "hook failed: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Aborted(e) | Error::Hook(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

impl From<crate::RowError> for Error {
    fn from(e: crate::RowError) -> Self {
        Error::Decode(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn display_and_retryable() {
        let e = Error::UniqueViolation {
            constraint: Some("user_email_key".into()),
        };
        assert_eq!(e.to_string(), "unique violation (user_email_key)");
        assert_eq!(
            Error::TableMissing {
                table: "user".into()
            }
            .to_string(),
            "table `user` does not exist — run `rango makemigrations` and `rango migrate`"
        );
        assert!(Error::Conflict.is_retryable());
        assert!(!Error::NotFound.is_retryable());
        assert_eq!(Error::abort("no funds").to_string(), "aborted: no funds");
        assert_eq!(Error::Hook("nope".into()).to_string(), "hook failed: nope");
    }
}
