//! Rust field type name → [`ColumnType`], shared by `#[derive(Model)]` and the CLI source scanner.
//! DB-free; not part of the user API.

use crate::schema::ColumnType;

/// Why a field type name has no column type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeMapError {
    /// Not a Rango field type.
    Unknown,
    /// A Rango generic field type whose parameters do not parse (`FieldVarchar<x, y>`).
    Invalid(&'static str),
}

/// Column type of a Rango field type text with its module path removed and spaces
/// stripped (`FieldVarchar<1,100>`). `ForeignKey<..>` is not handled here: its column
/// type follows the target's primary key, which only the caller can resolve.
pub fn column_type(type_str: &str) -> Result<ColumnType, TypeMapError> {
    Ok(match type_str {
        "FieldBool" => ColumnType::Bool,
        "FieldSmallInt" => ColumnType::SmallInt,
        "FieldInt" => ColumnType::Int,
        "FieldBigInt" => ColumnType::BigInt,
        "FieldAutoInt" => ColumnType::AutoInt,
        "FieldAutoBigInt" => ColumnType::AutoBigInt,
        "FieldFloat" => ColumnType::Float,
        "FieldDouble" => ColumnType::Double,
        "FieldText" => ColumnType::Text,
        "FieldEmail" => ColumnType::Varchar(254),
        "FieldUrl" => ColumnType::Varchar(2048),
        "FieldBytes" => ColumnType::Bytea,
        "FieldUuid" => ColumnType::Uuid,
        "FieldDate" => ColumnType::Date,
        "FieldTime" => ColumnType::Time,
        "FieldDateTime" => ColumnType::DateTime,
        "FieldJson" => ColumnType::Jsonb,
        s if s.starts_with("FieldVarchar<") => {
            let bad = TypeMapError::Invalid("FieldVarchar");
            let [_, max] = generics(s, "FieldVarchar<").ok_or(bad.clone())?;
            ColumnType::Varchar(max.parse().map_err(|_| bad)?)
        }
        s if s.starts_with("FieldPassword<") => {
            let bad = TypeMapError::Invalid("FieldPassword");
            let [_, max] = generics(s, "FieldPassword<").ok_or(bad.clone())?;
            max.parse::<u32>().map_err(|_| bad)?;
            ColumnType::Text
        }
        s if s.starts_with("FieldDecimal<") => {
            let bad = TypeMapError::Invalid("FieldDecimal");
            let [p, sc] = generics(s, "FieldDecimal<").ok_or(bad.clone())?;
            ColumnType::Decimal {
                precision: p.parse().map_err(|_| bad.clone())?,
                scale: sc.parse().map_err(|_| bad)?,
            }
        }
        s if s.starts_with("FieldRange<") => ColumnType::BigInt,
        _ => return Err(TypeMapError::Unknown),
    })
}

/// The two comma-separated generic arguments of `Name<a,b>`, trimmed.
fn generics<'a>(s: &'a str, prefix: &str) -> Option<[&'a str; 2]> {
    let inner = s.strip_prefix(prefix)?.strip_suffix('>')?;
    let mut it = inner.split(',').map(str::trim);
    match (it.next(), it.next(), it.next()) {
        (Some(a), Some(b), None) => Some([a, b]),
        _ => None,
    }
}

/// Suggestion naming the Rango field type to use instead of a plain Rust type.
pub fn rango_type_hint(type_str: &str) -> &'static str {
    match type_str {
        "String" | "&str" => "use `FieldText` or `FieldVarchar<MIN, MAX>`",
        "Vec<u8>" => "use `FieldBytes`",
        "bool" => "use `FieldBool`",
        "i16" => "use `FieldSmallInt`",
        "i32" => "use `FieldInt` (or `FieldAutoInt` for a database-assigned id)",
        "i64" => "use `FieldBigInt` (or `FieldAutoBigInt` for a database-assigned id)",
        "f32" => "use `FieldFloat`",
        "f64" => "use `FieldDouble`",
        "Uuid" | "uuid::Uuid" => "use `FieldUuid` (or `ForeignKey<Model>` for a reference)",
        "DateTime<Utc>" | "DateTime" => "use `FieldDateTime`",
        "NaiveDate" => "use `FieldDate`",
        "NaiveTime" => "use `FieldTime`",
        "Decimal" => "use `FieldDecimal<PRECISION, SCALE>`",
        "Value" => "use `FieldJson`",
        _ => "use one of the `Field*` types, `ForeignKey<Model>` or `ManyToMany<Model>`",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_plain_and_generic_types() {
        assert_eq!(column_type("FieldEmail"), Ok(ColumnType::Varchar(254)));
        assert_eq!(
            column_type("FieldVarchar<1,100>"),
            Ok(ColumnType::Varchar(100))
        );
        assert_eq!(column_type("FieldPassword<8,64>"), Ok(ColumnType::Text));
        assert_eq!(
            column_type("FieldDecimal<10,2>"),
            Ok(ColumnType::Decimal {
                precision: 10,
                scale: 2
            })
        );
        assert_eq!(column_type("FieldRange<1,5>"), Ok(ColumnType::BigInt));
        assert_eq!(column_type("String"), Err(TypeMapError::Unknown));
        assert_eq!(
            column_type("FieldVarchar<1>"),
            Err(TypeMapError::Invalid("FieldVarchar"))
        );
    }
}
