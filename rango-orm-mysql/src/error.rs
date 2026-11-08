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
            // ER_NO_SUCH_TABLE: "Table 'db.name' doesn't exist"
            let missing = db
                .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
                .filter(|m| m.number() == 1146)
                .and_then(|m| m.message().strip_prefix("Table '"))
                .and_then(|r| r.split_once('\''));
            if let Some((qualified, _)) = missing {
                return Error::TableMissing {
                    table: qualified.rsplit('.').next().unwrap_or(qualified).to_owned(),
                };
            }
            match code.as_deref() {
                // ER_LOCK_DEADLOCK (1213, SQLSTATE 40001) and ER_LOCK_WAIT_TIMEOUT (1205):
                // retrying the whole transaction can succeed.
                Some("40001") => Error::Conflict,
                Some("HY000") if mysql_number(db.as_ref()) == Some(1205) => Error::Conflict,
                // max_execution_time exceeded (MySQL / MariaDB)
                Some("HY000") if matches!(mysql_number(db.as_ref()), Some(3024 | 1969)) => {
                    Error::Timeout
                }
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

fn mysql_number(db: &dyn sqlx::error::DatabaseError) -> Option<u16> {
    db.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
        .map(|e| e.number())
}
