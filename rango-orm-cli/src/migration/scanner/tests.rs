use super::fields::map_field_type;
use super::scan_models;
use rango_orm_core::{ColumnType, Constraint};

fn ty(s: &str) -> syn::Type {
    syn::parse_str(s).expect("valid type string")
}

// ── map_field_type ─────────────────────────────────────────────────────────

#[test]
fn test_field_bool() {
    assert_eq!(map_field_type(&ty("FieldBool")).unwrap(), ColumnType::Bool);
}

#[test]
fn test_field_uuid() {
    assert_eq!(map_field_type(&ty("FieldUuid")).unwrap(), ColumnType::Uuid);
}

#[test]
fn test_field_text() {
    assert_eq!(map_field_type(&ty("FieldText")).unwrap(), ColumnType::Text);
}

#[test]
fn test_field_varchar_max() {
    assert_eq!(
        map_field_type(&ty("FieldVarchar<1, 100>")).unwrap(),
        ColumnType::Varchar(100),
    );
}

#[test]
fn test_field_password_maps_to_text() {
    assert_eq!(
        map_field_type(&ty("FieldPassword<8, 128>")).unwrap(),
        ColumnType::Text,
    );
}

#[test]
fn test_foreign_key_maps_to_uuid() {
    assert_eq!(
        map_field_type(&ty("ForeignKey<User>")).unwrap(),
        ColumnType::Uuid,
    );
}

#[test]
fn test_auto_pk_fields() {
    assert_eq!(
        map_field_type(&ty("FieldAutoInt")).unwrap(),
        ColumnType::AutoInt
    );
    assert_eq!(
        map_field_type(&ty("FieldAutoBigInt")).unwrap(),
        ColumnType::AutoBigInt
    );
}

#[test]
fn test_unknown_type_is_error() {
    assert!(map_field_type(&ty("SomeUnknownType")).is_err());
}

// ── scanner ───────────────────────────────────────────────────────────────

fn write_temp_models(name: &str, content: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rango_scanner_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("models.rs"), content).unwrap();
    dir
}

#[test]
fn test_scanner_fk_resolves_model_table() {
    let dir = write_temp_models(
        "fk_table",
        r#"
use rango_orm_core::Model;

#[derive(Model)]
#[model(table = "people")]
pub struct Person {
id: FieldUuid,
name: FieldText,
}

#[derive(Model)]
pub struct Profile {
id: FieldUuid,
person: ForeignKey<Person>,
}
"#,
    );

    let (schemas, _) = scan_models(dir.to_str().unwrap()).unwrap();
    let profile = schemas.iter().find(|s| s.table_name == "profile").unwrap();
    let col = profile.columns.iter().find(|c| c.name == "person").unwrap();
    let fk = col.references.as_ref().unwrap();
    assert_eq!(fk.table, "people");
    assert_eq!(fk.column, "id");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_scanner_invalid_on_delete_is_error() {
    let dir = write_temp_models(
        "bad_on_delete",
        r#"
use rango_orm_core::Model;

#[derive(Model)]
pub struct Person {
id: FieldUuid,
}

#[derive(Model)]
pub struct Profile {
id: FieldUuid,
#[field(on_delete = "bogus")]
person: ForeignKey<Person>,
}
"#,
    );

    let result = scan_models(dir.to_str().unwrap());
    assert!(result.is_err());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_scanner_constraints_ordering_and_m2m() {
    let dir = write_temp_models(
        "constraints_m2m",
        r#"
use rango_orm_core::Model;

#[derive(Model)]
#[model(
constraints = [
    CheckConstraint::new("age >= 0").name("chk_age"),
    UniqueConstraint::on(&["name", "age"]).name("uniq_name_age"),
],
ordering = [desc("age")]
)]
pub struct Person {
id: FieldUuid,
name: FieldText,
age: FieldInt,
}

#[derive(Model)]
pub struct Post {
id: FieldUuid,
readers: ManyToMany<Person>,
}
"#,
    );

    let (schemas, m2m) = scan_models(dir.to_str().unwrap()).unwrap();
    let person = schemas.iter().find(|s| s.table_name == "person").unwrap();
    assert_eq!(person.constraints.len(), 2);
    assert!(matches!(&person.constraints[0], Constraint::Check(c) if c.sql == "age >= 0"));
    assert!(matches!(&person.constraints[1], Constraint::Unique(u) if u.fields == ["name", "age"]));
    assert_eq!(person.ordering.len(), 1);
    assert_eq!(person.ordering[0].column, "age");

    let post = schemas.iter().find(|s| s.table_name == "post").unwrap();
    assert!(post.columns.iter().all(|c| c.name != "readers"));
    assert_eq!(m2m.len(), 1);
    assert_eq!(m2m[0].to_table, "person");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_scanner_implicit_pk_attribute_macro() {
    let dir = write_temp_models(
        "implicit_pk",
        r#"
use rango::prelude::*;

#[rango::model]
#[model(table = "art")]
pub struct Article {
title: FieldText,
}

#[rango::model(pk = "bigint")]
pub struct Event {
name: FieldText,
}

#[model]
pub struct Plain {
name: FieldText,
}

#[rango::model]
pub struct Keeps {
id: FieldAutoBigInt,
}

#[rango::model]
pub struct Custom {
#[field(primary_key)]
code: FieldText,
}

#[derive(Model)]
#[model(table = "helper_only")]
pub struct Helper {
id: FieldUuid,
}
"#,
    );

    let (schemas, _) = scan_models(dir.to_str().unwrap()).unwrap();
    let col = |t: &str| {
        let s = schemas.iter().find(|s| s.table_name == t).unwrap();
        (s.columns.len(), s.columns[0].clone())
    };
    let (n, id) = col("art");
    assert_eq!((n, id.name.as_str(), id.primary_key), (2, "id", true));
    assert_eq!(id.col_type, ColumnType::Uuid);
    assert_eq!(col("event").1.col_type, ColumnType::AutoBigInt);
    assert_eq!(col("plain").1.col_type, ColumnType::Uuid);
    assert_eq!(col("keeps").0, 1);
    assert_eq!(col("custom").0, 1);
    assert_eq!(col("helper_only").0, 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_scanner_m2m_uses_explicit_tables_and_runtime_pivot_name() {
    let dir = write_temp_models(
        "m2m_explicit",
        r#"
use rango_orm_core::Model;

#[derive(Model)]
#[model(table = "people")]
pub struct Person {
id: FieldUuid,
}

#[derive(Model)]
#[model(table = "articles")]
pub struct Post {
id: FieldUuid,
readers: ManyToMany<Person>,
}
"#,
    );
    let (_, m2m) = scan_models(dir.to_str().unwrap()).unwrap();
    let pivot = super::generate_pivot_table(&m2m[0], &[]);
    assert_eq!(pivot.table_name, "articles_people");
    assert_eq!(pivot.columns[0].name, "articles_id");
    assert_eq!(
        pivot.columns[1].references.as_ref().unwrap().table,
        "people"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fk_and_pivot_follow_the_target_primary_key() {
    let dir = std::env::temp_dir().join(format!("rango-fk-pk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("models.rs"),
        r#"
use rango_orm_core::Model;

#[derive(Model)]
#[model(table = "authors")]
pub struct Author {
id: FieldAutoBigInt,
}

#[derive(Model)]
#[model(table = "tags")]
pub struct Tag {
id: FieldAutoInt,
}

#[derive(Model)]
#[model(table = "owners")]
pub struct Owner {
id: FieldUuid,
}

#[derive(Model)]
#[model(table = "posts")]
pub struct Post {
id: FieldUuid,
author_id: ForeignKey<Author>,
owner_id: ForeignKey<Owner>,
editor_id: Option<ForeignKey<Tag>>,
tags: ManyToMany<Tag>,
authors: ManyToMany<Author>,
}
"#,
    )
    .unwrap();
    let (schemas, m2m) = scan_models(dir.to_str().unwrap()).unwrap();
    let posts = schemas.iter().find(|s| s.table_name == "posts").unwrap();
    let ty = |name: &str| {
        posts
            .columns
            .iter()
            .find(|c| c.name == name)
            .unwrap()
            .col_type
            .clone()
    };
    assert_eq!(ty("author_id"), ColumnType::BigInt);
    assert_eq!(ty("owner_id"), ColumnType::Uuid);
    assert_eq!(ty("editor_id"), ColumnType::Int);

    let pivot_of = |to: &str| {
        let rel = m2m.iter().find(|r| r.to_table == to).unwrap();
        super::generate_pivot_table(rel, &schemas)
    };
    let tags = pivot_of("tags");
    assert_eq!(tags.columns[0].col_type, ColumnType::Uuid);
    assert_eq!(tags.columns[1].col_type, ColumnType::Int);
    let authors = pivot_of("authors");
    assert_eq!(authors.columns[1].col_type, ColumnType::BigInt);
    let _ = std::fs::remove_dir_all(&dir);
}
