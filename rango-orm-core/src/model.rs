use crate::schema::TableSchema;

/// Core trait implemented by `#[derive(Model)]`.
/// Every model must be able to describe its own schema.
/// `ModelHooks` is a supertrait: the derive emits an empty impl unless `#[model(hooks)]` is set.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a Rango model",
    label = "this type does not implement `Model`",
    note = "add `#[derive(Model)]` (or `#[rango::model]`) to `{Self}`"
)]
pub trait Model: crate::ModelHooks {
    /// The Rust type of the primary key value (`Uuid`, `i32`, `i64`).
    /// `ForeignKey<Self>` and `ManyToMany<Self>` columns follow it.
    type Pk: PrimaryKey;

    /// The table name in the database.
    fn table_name() -> &'static str;

    /// The full schema description for this model.
    fn schema() -> TableSchema;
}

/// A primary key value type that a `ForeignKey<M>` column can hold: `Uuid` (the default),
/// `i32` (`FieldAutoInt` / `FieldInt`) or `i64` (`FieldAutoBigInt` / `FieldBigInt`).
pub trait PrimaryKey:
    Copy
    + Eq
    + std::hash::Hash
    + std::fmt::Debug
    + Send
    + Sync
    + 'static
    + crate::ToSqlValue
    + serde::Serialize
    + serde::de::DeserializeOwned
{
    /// Type of a foreign key column pointing at this key.
    const COLUMN_TYPE: crate::ColumnType;

    /// The SQL NULL of this type (for `Option<ForeignKey<M>>`).
    fn null_value() -> crate::SqlValue;

    /// Reads the key from `col`.
    fn from_row(row: &dyn crate::RangoRow, col: &str) -> Result<Self, crate::RowError>;
}

impl PrimaryKey for uuid::Uuid {
    const COLUMN_TYPE: crate::ColumnType = crate::ColumnType::Uuid;
    fn null_value() -> crate::SqlValue {
        crate::SqlValue::NullUuid
    }
    fn from_row(row: &dyn crate::RangoRow, col: &str) -> Result<Self, crate::RowError> {
        row.get_uuid(col)
    }
}

impl PrimaryKey for i32 {
    const COLUMN_TYPE: crate::ColumnType = crate::ColumnType::Int;
    fn null_value() -> crate::SqlValue {
        crate::SqlValue::NullInt
    }
    fn from_row(row: &dyn crate::RangoRow, col: &str) -> Result<Self, crate::RowError> {
        row.get_i32(col)
    }
}

impl PrimaryKey for i64 {
    const COLUMN_TYPE: crate::ColumnType = crate::ColumnType::BigInt;
    fn null_value() -> crate::SqlValue {
        crate::SqlValue::NullBigInt
    }
    fn from_row(row: &dyn crate::RangoRow, col: &str) -> Result<Self, crate::RowError> {
        row.get_i64(col)
    }
}

/// The key type of a primary key field type: `FieldUuid` -> `Uuid`, `FieldAutoInt` and
/// `FieldInt` -> `i32`, `FieldAutoBigInt` and `FieldBigInt` -> `i64`.
/// Used by `#[derive(Model)]` to set `Model::Pk`.
pub trait PrimaryKeyField {
    type Pk: PrimaryKey;
}

impl PrimaryKeyField for crate::FieldUuid {
    type Pk = uuid::Uuid;
}
impl PrimaryKeyField for crate::FieldInt {
    type Pk = i32;
}
impl PrimaryKeyField for crate::FieldAutoInt {
    type Pk = i32;
}
impl PrimaryKeyField for crate::FieldBigInt {
    type Pk = i64;
}
impl PrimaryKeyField for crate::FieldAutoBigInt {
    type Pk = i64;
}

/// `Model::Pk` of a model whose primary key is none of the types above (a text key, say).
/// Nothing can build one, so `ForeignKey<Model>` is unusable for it. Set by the derive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum UnsupportedPk {}

impl crate::ToSqlValue for UnsupportedPk {
    fn to_sql_value(&self) -> crate::SqlValue {
        match *self {}
    }
}

impl PrimaryKey for UnsupportedPk {
    const COLUMN_TYPE: crate::ColumnType = crate::ColumnType::Uuid;
    fn null_value() -> crate::SqlValue {
        crate::SqlValue::NullUuid
    }
    fn from_row(_: &dyn crate::RangoRow, col: &str) -> Result<Self, crate::RowError> {
        Err(crate::RowError::new(format!(
            "{col}: the target model has no UUID, integer or bigint primary key"
        )))
    }
}

/// Marker trait for abstract model mixins (`#[derive(ModelMixin)]`).
/// A mixin is never a real table — its fields get flattened into models that use it.
pub trait ModelMixin {
    fn mixin_name() -> &'static str;
}

/// Defense in depth for column names passed as strings (`.eq("email", ..)`): an unknown
/// column becomes a typed error before any SQL runs, instead of a database error.
/// Returns the message for `Error::Invalid` so query builders can defer it to execution.
#[doc(hidden)]
pub fn unknown_column<M: Model>(name: &str) -> Option<String> {
    // ponytail: builds M::schema() per call; cache the column list if it shows up in profiles.
    let schema = M::schema();
    if schema.columns.iter().any(|c| c.name == name) {
        return None;
    }
    let hint = schema
        .columns
        .iter()
        .map(|c| (edit_distance(name, &c.name), &c.name))
        .min()
        // Close enough to be a typo: at most a third of the name.
        .filter(|(d, c)| *d <= c.len().max(name.len()).div_ceil(3))
        .map(|(_, c)| format!(", did you mean `{c}`?"))
        .unwrap_or_default();
    Some(format!(
        "unknown column `{name}` on table `{}`{hint}",
        M::table_name()
    ))
}

/// Check that every name is a column of `M`; `Error::Invalid` otherwise (before any SQL runs).
#[doc(hidden)]
pub fn check_columns<'a, M: Model>(names: impl IntoIterator<Item = &'a str>) -> crate::Result<()> {
    for n in names {
        if let Some(msg) = unknown_column::<M>(n) {
            return Err(crate::Error::Invalid(msg));
        }
    }
    Ok(())
}

/// Levenshtein distance, two rows.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != *cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod fuzz_tests {
    //! Fuzz tests for identifier quoting and exact column matching.

    use super::{Model, unknown_column};
    use crate::query::{quote_ident, quote_ident_mysql};
    use crate::schema::{ColumnDef, ColumnType, TableSchema};

    struct XorShift64(u64);

    impl XorShift64 {
        fn new(seed: u64) -> Self {
            Self(seed)
        }

        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
    }

    const SEED: u64 = 0x1234_5678_9abc_def0;

    const ALPHABET: [char; 18] = [
        '"', '`', '\'', ';', '-', '/', '*', '\\', ' ', '\n', '\0', 'a', 'Z', '_', 'é', '€',
        '\u{FF02}', '\u{202E}',
    ];

    fn random_string(rng: &mut XorShift64, max_len: usize) -> String {
        let len = (rng.next_u64() % max_len as u64) as usize;
        (0..len)
            .map(|_| ALPHABET[(rng.next_u64() % ALPHABET.len() as u64) as usize])
            .collect()
    }

    fn unquote(s: &str, q: char) -> Option<String> {
        let mut chars = s.chars();
        if chars.next()? != q {
            return None;
        }

        let mut out = String::new();
        loop {
            match chars.next()? {
                c if c == q => match chars.next() {
                    Some(q2) if q2 == q => out.push(q),
                    None => return Some(out),
                    Some(_) => return None,
                },
                c => out.push(c),
            }
        }
    }

    struct TestModel;

    impl crate::ModelHooks for TestModel {}

    impl Model for TestModel {
        type Pk = uuid::Uuid;

        fn table_name() -> &'static str {
            "test_models"
        }

        fn schema() -> TableSchema {
            TableSchema {
                table_name: "test_models".to_string(),
                columns: vec![
                    column("id", true, false, true),
                    column("email", false, true, true),
                    column("created_at", false, false, false),
                ],
                ..TableSchema::default()
            }
        }
    }

    fn column(name: &str, primary_key: bool, unique: bool, index: bool) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            col_type: ColumnType::Text,
            nullable: false,
            primary_key,
            unique,
            default: None,
            references: None,
            index,
        }
    }

    #[test]
    fn quote_ident_is_one_token_roundtrip() {
        let mut rng = XorShift64::new(SEED);
        for _ in 0..10_000 {
            let s = random_string(&mut rng, 32);
            assert_eq!(
                unquote(&quote_ident(&s), '"'),
                Some(s.clone()),
                "quote_ident roundtrip failed for input {:?}",
                s
            );
            assert_eq!(
                unquote(&quote_ident_mysql(&s), '`'),
                Some(s.clone()),
                "quote_ident_mysql roundtrip failed for input {:?}",
                s
            );
        }
    }

    #[test]
    fn unknown_column_suggests_close_names() {
        let msg = unknown_column::<TestModel>("emial").unwrap();
        assert!(msg.ends_with("did you mean `email`?"), "{msg}");
        let msg = unknown_column::<TestModel>("zzzzzz").unwrap();
        assert!(!msg.contains("did you mean"), "{msg}");
    }

    #[test]
    fn unknown_column_only_accepts_exact_names() {
        for name in ["id", "email", "created_at"] {
            assert_eq!(
                unknown_column::<TestModel>(name),
                None,
                "exact column name should be known: {:?}",
                name
            );
        }

        let mut rng = XorShift64::new(SEED);
        for _ in 0..10_000 {
            let s = random_string(&mut rng, 32);
            if s != "id" && s != "email" && s != "created_at" {
                assert!(
                    unknown_column::<TestModel>(&s).is_some(),
                    "non-exact name should be unknown: {:?}",
                    s
                );
            }
        }

        let near_misses = [
            "Email",
            "email ",
            " email",
            "email\0",
            "\"email\"",
            "email;--",
            "email\u{200B}",
            "ema\u{456}l",
            "",
        ];
        for name in near_misses {
            assert!(
                unknown_column::<TestModel>(name).is_some(),
                "near-miss name should be unknown: {:?}",
                name
            );
        }
    }
}
