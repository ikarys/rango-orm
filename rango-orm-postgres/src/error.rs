use rango_orm_core::Error;
use sqlx::error::ErrorKind;

/// Translate a sqlx error into a typed [`Error`]. Never includes SQL text or bound values.
pub(crate) fn db_err(e: sqlx::Error) -> Error {
    match e {
        sqlx::Error::RowNotFound => Error::NotFound,
        sqlx::Error::PoolTimedOut => Error::Timeout,
        sqlx::Error::Database(db) => {
            let constraint = db.constraint().map(str::to_owned);
            match db.kind() {
                ErrorKind::UniqueViolation => return Error::UniqueViolation { constraint },
                ErrorKind::ForeignKeyViolation => return Error::ForeignKeyViolation { constraint },
                ErrorKind::NotNullViolation => return Error::NotNullViolation { constraint },
                ErrorKind::CheckViolation => return Error::CheckViolation { constraint },
                _ => {}
            }
            let code = db.code().map(|c| c.into_owned());
            match code.as_deref() {
                Some("40001" | "40P01") => Error::Conflict,
                // query_canceled (statement_timeout), lock_not_available (lock_timeout)
                Some("57014" | "55P03") => Error::Timeout,
                _ => Error::Database {
                    code,
                    message: db.message().to_owned(),
                },
            }
        }
        sqlx::Error::Configuration(e) => Error::Invalid(e.to_string()),
        sqlx::Error::ColumnDecode { .. }
        | sqlx::Error::Decode(_)
        | sqlx::Error::ColumnNotFound(_)
        | sqlx::Error::ColumnIndexOutOfBounds { .. }
        | sqlx::Error::TypeNotFound { .. } => Error::Decode(e.to_string()),
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => Error::Connection(e.to_string()),
        other => Error::Database {
            code: None,
            message: other.to_string(),
        },
    }
}
