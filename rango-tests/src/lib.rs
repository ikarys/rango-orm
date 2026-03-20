use rango_core::*;
use rango_derive::{Model, ModelMixin};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_name_auto() {
        assert_eq!(Article::table_name(), "article");
    }

    #[test]
    fn test_table_name_override() {
        assert_eq!(UserAccount::table_name(), "user_accounts");
    }

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

        let created = schema.columns.iter().find(|c| c.name == "created_at").unwrap();
        assert!(matches!(created.default, Some(DefaultValue::CurrentTimestamp)));
    }
}
