use crate::schema::TableSchema;

/// Core trait implemented by `#[derive(Model)]`.
/// Every model must be able to describe its own schema.
/// `ModelHooks` is a supertrait: the derive emits an empty impl unless `#[model(hooks)]` is set.
pub trait Model: crate::ModelHooks {
    /// The table name in the database.
    fn table_name() -> &'static str;

    /// The full schema description for this model.
    fn schema() -> TableSchema;
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
    if M::schema().columns.iter().any(|c| c.name == name) {
        None
    } else {
        Some(format!(
            "unknown column `{name}` on table `{}`",
            M::table_name()
        ))
    }
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
