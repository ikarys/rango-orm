use rango_orm_core::Error;
use sqlx::error::ErrorKind;

/// Translate a sqlx error into a typed [`Error`]. Never includes SQL text or bound values.
pub(crate) fn db_err(e: sqlx::Error) -> Error {
    match e {
        sqlx::Error::RowNotFound => Error::NotFound,
        sqlx::Error::PoolTimedOut => Error::Timeout,
        sqlx::Error::Database(db) => {
            let kind = db.kind();
            let constraint = constraint_name(&kind, db.message());
            match kind {
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

/// Constraint name from the server message (sqlx reports none on MySQL / MariaDB). Only the
/// name is extracted: the offending value and the SQL never reach the error.
///
/// - unique: `Duplicate entry '<v>' for key 'tbl.name'` (MySQL 8, table prefix dropped) or `... for key 'name'` (MariaDB)
/// - foreign key: `... foreign key constraint fails (`db`.`t`, CONSTRAINT `name` FOREIGN KEY ...)`
/// - check: `Check constraint 'name' is violated.` (MySQL) or ``CONSTRAINT `name` failed for ...`` (MariaDB)
fn constraint_name(kind: &ErrorKind, msg: &str) -> Option<String> {
    let name = match kind {
        ErrorKind::UniqueViolation => {
            // `rfind`: the value comes first and may itself contain the marker.
            let rest = &msg[msg.rfind(" for key '")? + " for key '".len()..];
            let key = rest.strip_suffix('\'')?;
            // ponytail: a MariaDB key name containing a dot loses its prefix; no fix needed unless seen.
            key.split_once('.').map_or(key, |(_, k)| k)
        }
        ErrorKind::ForeignKeyViolation => between(msg, "CONSTRAINT `", "`")?,
        ErrorKind::CheckViolation => {
            between(msg, "Check constraint '", "'").or_else(|| between(msg, "CONSTRAINT `", "`"))?
        }
        _ => return None,
    };
    Some(name.to_owned())
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let rest = &s[s.find(start)? + start.len()..];
    Some(&rest[..rest.find(end)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_names() {
        let k = ErrorKind::UniqueViolation;
        // MySQL 8.4 prefixes the table
        assert_eq!(
            constraint_name(
                &k,
                "Duplicate entry 'a@b.c' for key 'attendee.attendee_email_unique'"
            ),
            Some("attendee_email_unique".into())
        );
        // MariaDB 11.4
        assert_eq!(
            constraint_name(
                &k,
                "Duplicate entry 'a@b.c' for key 'attendee_email_unique'"
            ),
            Some("attendee_email_unique".into())
        );
        // a value that looks like the marker is not mistaken for the key
        assert_eq!(
            constraint_name(&k, "Duplicate entry 'x for key 'y' for key 'attendee.uq'"),
            Some("uq".into())
        );
        assert_eq!(constraint_name(&k, "garbage"), None);
    }

    #[test]
    fn foreign_key_names() {
        let k = ErrorKind::ForeignKeyViolation;
        for msg in [
            "Cannot add or update a child row: a foreign key constraint fails (`db`.`post`, CONSTRAINT `post_author_fk` FOREIGN KEY (`author_id`) REFERENCES `user` (`id`))",
            "Cannot delete or update a parent row: a foreign key constraint fails (`db`.`post`, CONSTRAINT `post_author_fk` FOREIGN KEY (`author_id`) REFERENCES `user` (`id`) ON DELETE RESTRICT)",
        ] {
            assert_eq!(constraint_name(&k, msg), Some("post_author_fk".into()));
        }
    }

    #[test]
    fn check_names() {
        let k = ErrorKind::CheckViolation;
        assert_eq!(
            constraint_name(&k, "Check constraint 'chk_age' is violated."),
            Some("chk_age".into())
        );
        assert_eq!(
            constraint_name(&k, "CONSTRAINT `chk_age` failed for `db`.`person`"),
            Some("chk_age".into())
        );
        assert_eq!(constraint_name(&ErrorKind::NotNullViolation, "x"), None);
    }
}
