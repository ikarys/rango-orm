#![allow(dead_code)]

use rango_core::*;
use rango_derive::{Model, ModelMixin};

// ── Existing models ────────────────────────────────────────────────────────────

#[derive(ModelMixin)]
struct Timestamps {
    #[field(auto_now_add)]
    created_at: FieldDateTime,
    #[field(auto_now)]
    updated_at: FieldDateTime,
}

#[derive(Model)]
struct Article {
    id: FieldUuid,
    #[field(unique, index)]
    slug: FieldVarchar<1, 100>,
    title: FieldVarchar<1, 255>,
    body: Option<FieldText>,
    published: FieldBool,
}

#[derive(Model)]
#[model(table = "user_accounts")]
struct UserAccount {
    id: FieldUuid,
    #[field(unique)]
    email: FieldEmail,
    password: FieldPassword<8, 128>,
    age: Option<FieldRange<0, 150>>,
    #[field(auto_now_add)]
    created_at: FieldDateTime,
}

// ── New models for extended tests ──────────────────────────────────────────────

/// Model whose PK is not named "id" — uses #[field(primary_key)] instead.
#[derive(Model)]
struct WithCustomPk {
    #[field(primary_key)]
    uuid: FieldUuid,
    name: FieldText,
}

/// Model with a ForeignKey field.
#[derive(Model)]
struct Comment {
    id: FieldUuid,
    article_id: ForeignKey<Article>,
    body: FieldText,
}

/// Model with managed = false.
#[derive(Model)]
#[model(managed = false)]
struct UnmanagedThing {
    id: FieldUuid,
    label: FieldText,
}

/// Model with ordering and constraints.
#[derive(Model)]
#[model(
    ordering = [asc("name"), desc("created_at")],
    constraints = [
        CheckConstraint::new("score > 0").name("score_positive"),
        UniqueConstraint::on(&["name"]).name("name_unique"),
    ]
)]
struct RichModel {
    id: FieldUuid,
    name: FieldVarchar<1, 100>,
    score: FieldInt,
    created_at: FieldDateTime,
}

/// Model for testing field attribute: index and column override.
#[derive(Model)]
struct FieldAttrModel {
    id: FieldUuid,
    #[field(index)]
    indexed_col: FieldText,
    #[field(column = "custom_col")]
    renamed: FieldText,
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Table name detection ───────────────────────────────────────────────────

    #[test]
    fn test_table_name_auto() {
        assert_eq!(Article::table_name(), "article");
    }

    #[test]
    fn test_table_name_override() {
        assert_eq!(UserAccount::table_name(), "user_accounts");
    }

    // ── PK detection: id field ─────────────────────────────────────────────────

    #[test]
    fn test_article_id_is_pk() {
        let schema = Article::schema();
        let id = schema.columns.iter().find(|c| c.name == "id").unwrap();
        assert!(id.primary_key, "id column should be primary_key");
        assert!(!id.nullable, "id column should not be nullable");
        assert_eq!(id.col_type, ColumnType::Uuid);
    }

    #[test]
    fn test_pk_column_default_id() {
        assert_eq!(Article::pk_column(), "id");
    }

    // ── PK detection: custom #[field(primary_key)] ─────────────────────────────

    #[test]
    fn test_custom_pk_field_is_pk() {
        let schema = WithCustomPk::schema();
        let col = schema.columns.iter().find(|c| c.name == "uuid").unwrap();
        assert!(col.primary_key, "uuid column should be primary_key");
        assert!(!col.nullable);
        assert_eq!(col.col_type, ColumnType::Uuid);
    }

    #[test]
    fn test_custom_pk_column_name() {
        assert_eq!(WithCustomPk::pk_column(), "uuid");
    }

    // ── ForeignKey<M> column type ──────────────────────────────────────────────

    #[test]
    fn test_foreign_key_column_type_is_uuid() {
        let schema = Comment::schema();
        let col = schema
            .columns
            .iter()
            .find(|c| c.name == "article_id")
            .unwrap();
        assert_eq!(
            col.col_type,
            ColumnType::Uuid,
            "FK should map to ColumnType::Uuid"
        );
        assert!(!col.nullable, "FK column should not be nullable");
    }

    // ── Nullable / non-nullable ────────────────────────────────────────────────

    #[test]
    fn test_nullable_option_field() {
        let schema = Article::schema();
        let body = schema.columns.iter().find(|c| c.name == "body").unwrap();
        assert!(body.nullable, "Option<FieldText> should be nullable");
    }

    #[test]
    fn test_non_nullable_field() {
        let schema = Article::schema();
        let title = schema.columns.iter().find(|c| c.name == "title").unwrap();
        assert!(!title.nullable, "FieldVarchar should not be nullable");
    }

    // ── Field attributes: unique ───────────────────────────────────────────────

    #[test]
    fn test_field_unique() {
        let schema = Article::schema();
        let slug = schema.columns.iter().find(|c| c.name == "slug").unwrap();
        assert!(slug.unique, "slug should have unique = true");
    }

    #[test]
    fn test_non_unique_field() {
        let schema = Article::schema();
        let title = schema.columns.iter().find(|c| c.name == "title").unwrap();
        assert!(!title.unique, "title should not be unique");
    }

    // ── Field attributes: index (attribute parsed; column still appears in schema) ──

    #[test]
    fn test_field_index_column_in_schema() {
        let schema = FieldAttrModel::schema();
        assert!(
            schema.columns.iter().any(|c| c.name == "indexed_col"),
            "indexed_col should appear in schema",
        );
    }

    // ── Field attributes: column name override ────────────────────────────────

    #[test]
    fn test_field_column_rename() {
        let schema = FieldAttrModel::schema();
        // The Rust field is `renamed` but column = "custom_col"
        assert!(
            schema.columns.iter().any(|c| c.name == "custom_col"),
            "custom_col override should appear in schema",
        );
        assert!(
            !schema.columns.iter().any(|c| c.name == "renamed"),
            "original rust field name should NOT appear as column",
        );
    }

    // ── Full schema shape ──────────────────────────────────────────────────────

    #[test]
    fn test_article_schema() {
        let schema = Article::schema();
        assert_eq!(schema.table_name, "article");
        assert_eq!(schema.columns.len(), 5);

        let id = schema.columns.iter().find(|c| c.name == "id").unwrap();
        assert!(id.primary_key);
        assert!(!id.nullable);

        let slug = schema.columns.iter().find(|c| c.name == "slug").unwrap();
        assert!(slug.unique);
        assert!(!slug.nullable);

        let body = schema.columns.iter().find(|c| c.name == "body").unwrap();
        assert!(body.nullable);
    }

    #[test]
    fn test_user_schema() {
        let schema = UserAccount::schema();
        assert_eq!(schema.table_name, "user_accounts");

        let email = schema.columns.iter().find(|c| c.name == "email").unwrap();
        assert!(email.unique);
        assert_eq!(email.col_type, ColumnType::Varchar(254));

        let created = schema
            .columns
            .iter()
            .find(|c| c.name == "created_at")
            .unwrap();
        assert!(matches!(
            created.default,
            Some(DefaultValue::CurrentTimestamp)
        ));
    }

    // ── managed = false ────────────────────────────────────────────────────────

    #[test]
    fn test_managed_false() {
        let schema = UnmanagedThing::schema();
        assert!(!schema.managed, "schema.managed should be false");
    }

    #[test]
    fn test_managed_true_default() {
        let schema = Article::schema();
        assert!(schema.managed, "schema.managed defaults to true");
    }

    // ── ordering ───────────────────────────────────────────────────────────────

    #[test]
    fn test_ordering_entries() {
        let schema = RichModel::schema();
        assert_eq!(schema.ordering.len(), 2, "should have 2 ordering entries");
        assert_eq!(schema.ordering[0].column, "name");
        assert_eq!(schema.ordering[0].dir, OrderDir::Asc);
        assert_eq!(schema.ordering[1].column, "created_at");
        assert_eq!(schema.ordering[1].dir, OrderDir::Desc);
    }

    // ── constraints ────────────────────────────────────────────────────────────

    #[test]
    fn test_check_constraint_in_schema() {
        let schema = RichModel::schema();
        let has_check = schema
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::Check(_)));
        assert!(has_check, "schema should contain a CheckConstraint");
        if let Some(Constraint::Check(cc)) = schema
            .constraints
            .iter()
            .find(|c| matches!(c, Constraint::Check(_)))
        {
            assert_eq!(cc.sql, "score > 0");
            assert_eq!(cc.name, "score_positive");
        }
    }

    #[test]
    fn test_unique_constraint_in_schema() {
        let schema = RichModel::schema();
        let has_unique = schema
            .constraints
            .iter()
            .any(|c| matches!(c, Constraint::Unique(_)));
        assert!(has_unique, "schema should contain a UniqueConstraint");
        if let Some(Constraint::Unique(uc)) = schema
            .constraints
            .iter()
            .find(|c| matches!(c, Constraint::Unique(_)))
        {
            assert_eq!(uc.fields, vec!["name".to_string()]);
            assert_eq!(uc.name, "name_unique");
            assert!(uc.condition.is_none());
        }
    }

    // ── ModelMixin ─────────────────────────────────────────────────────────────

    #[test]
    fn test_model_mixin_name() {
        assert_eq!(Timestamps::mixin_name(), "Timestamps");
    }

    #[test]
    fn test_mixin_fields_in_user_account() {
        // UserAccount manually includes a created_at field (mixin-style pattern)
        let schema = UserAccount::schema();
        assert!(
            schema.columns.iter().any(|c| c.name == "created_at"),
            "mixin-style field created_at should appear in schema",
        );
        let created = schema
            .columns
            .iter()
            .find(|c| c.name == "created_at")
            .unwrap();
        assert_eq!(created.col_type, ColumnType::DateTime);
        assert!(matches!(
            created.default,
            Some(DefaultValue::CurrentTimestamp)
        ));
    }
}

#[cfg(test)]
mod query_tests;
