//! Shared fixture: one realistic set of models, used three ways.
//!
//! 1. `contract.rs` compiles it and runs the real `rango makemigrations` on this same file,
//!    then compares what the derive says with what the CLI scanner says.
//! 2. `real_project_*.rs` migrate it through the CLI and run queries with the compiled models.
//! 3. The CLI scanner reads it as plain text, so keep to what a real project would write.
//!
//! Tables are named `rpx_*` so they never collide with other tests that share one database.
#![allow(dead_code)]

use rango::prelude::*;

/// Marker only: must never become a table.
#[derive(ModelMixin)]
pub struct RpxTimestamps {
    created_at: FieldDateTime,
}

/// Explicit table name, unique + index + renamed column + auto_now_add + ordering.
#[derive(Model, Debug, Clone)]
#[model(table = "rpx_authors", ordering = [asc("name")])]
pub struct RpxAuthor {
    pub id: FieldUuid,
    #[field(unique)]
    pub email: FieldEmail,
    #[field(index)]
    pub name: FieldVarchar<1, 100>,
    pub bio: Option<FieldText>,
    pub karma: Option<FieldInt>,
    #[field(column = "joined_at", auto_now_add)]
    pub created: FieldDateTime,
    /// Self-referencing M2M: pivot `rpx_authors_rpx_authors` with `from_rpx_authors_id` /
    /// `to_rpx_authors_id`.
    pub followers: ManyToMany<RpxAuthor>,
}

/// Default table name from a multi-word struct name (`rpx_blog_post`), FK to a model
/// with an explicit table, a second nullable FK to it, M2M to a model with an explicit
/// table, and both constraint kinds.
#[derive(Model, Debug, Clone)]
#[model(
    constraints = [
        CheckConstraint::new("rating >= 0").name("chk_rpx_rating"),
        UniqueConstraint::on(&["author", "title"]).name("uniq_rpx_author_title"),
    ]
)]
pub struct RpxBlogPost {
    pub id: FieldUuid,
    pub author: ForeignKey<RpxAuthor>,
    #[field(on_delete = "set_null")]
    pub reviewer: Option<ForeignKey<RpxAuthor>>,
    pub title: FieldVarchar<1, 200>,
    #[field(column = "body_text")]
    pub body: FieldText,
    pub published: FieldBool,
    pub rating: FieldInt,
    pub tags: ManyToMany<RpxTag>,
}

#[derive(Model, Debug, Clone)]
#[model(table = "rpx_tags")]
pub struct RpxTag {
    pub id: FieldUuid,
    #[field(unique)]
    pub name: FieldVarchar<1, 50>,
}

/// Self reference.
#[derive(Model, Debug, Clone)]
pub struct RpxCategory {
    pub id: FieldUuid,
    pub name: FieldText,
    #[field(on_delete = "set_null")]
    pub parent: Option<ForeignKey<RpxCategory>>,
}

/// Foreign key cycle: A -> B -> A, both nullable.
#[derive(Model, Debug, Clone)]
pub struct RpxNodeA {
    pub id: FieldUuid,
    pub label: FieldText,
    #[field(on_delete = "set_null")]
    pub b: Option<ForeignKey<RpxNodeB>>,
}

#[derive(Model, Debug, Clone)]
pub struct RpxNodeB {
    pub id: FieldUuid,
    pub label: FieldText,
    #[field(on_delete = "set_null")]
    pub a: Option<ForeignKey<RpxNodeA>>,
}

/// Implicit primary key, uuid and bigint.
#[rango::model]
#[derive(Debug, Clone)]
pub struct RpxNote {
    pub title: FieldText,
}

#[rango::model(pk = "bigint")]
#[derive(Debug, Clone)]
pub struct RpxCounter {
    pub name: FieldVarchar<1, 50>,
    pub hits: FieldBigInt,
}

/// Explicit database-generated primary key.
#[derive(Model, Debug, Clone)]
#[model(table = "rpx_events")]
pub struct RpxEvent {
    pub id: FieldAutoBigInt,
    pub kind: FieldText,
}

/// `on_delete = "restrict"`: the author cannot be deleted while a pin points at it.
#[derive(Model, Debug, Clone)]
#[model(table = "rpx_pins")]
pub struct RpxPin {
    pub id: FieldUuid,
    #[field(on_delete = "restrict")]
    pub author: ForeignKey<RpxAuthor>,
}

/// Not managed by migrations.
#[derive(Model, Debug, Clone)]
#[model(table = "rpx_legacy", managed = false)]
pub struct RpxLegacy {
    pub id: FieldUuid,
    pub label: FieldText,
}

/// Field types and derives written with a path, the way `rango::...` users write them.
#[derive(rango::Model, Debug, Clone)]
pub struct RpxPathTyped {
    pub id: rango::FieldUuid,
    pub label: rango::FieldText,
    pub count: Option<rango::FieldInt>,
    pub author: rango::ForeignKey<RpxAuthor>,
}

/// One column of every field type.
#[derive(Model, Debug, Clone)]
pub struct RpxSink {
    pub id: FieldUuid,
    pub b: FieldBool,
    pub small: FieldSmallInt,
    pub int: FieldInt,
    pub big: FieldBigInt,
    pub float: FieldFloat,
    pub double: FieldDouble,
    pub dec: FieldDecimal<10, 2>,
    pub text: FieldText,
    pub short: FieldVarchar<1, 20>,
    pub email: FieldEmail,
    pub url: FieldUrl,
    pub bytes: FieldBytes,
    pub day: FieldDate,
    pub at: FieldTime,
    pub stamp: FieldDateTime,
    pub json: FieldJson,
    pub range: FieldRange<0, 100>,
    pub secret: FieldPassword<8, 64>,
}

/// Partial select of `RpxAuthor` (note the renamed column).
#[derive(Partial, Debug, PartialEq)]
#[partial(model = RpxAuthor)]
pub struct RpxAuthorLite {
    pub name: FieldVarchar<1, 100>,
    pub bio: Option<FieldText>,
}

/// Every model, as the derive sees it. `contract.rs` checks that this list and the CLI
/// scanner agree on the set of tables, so a model missing here fails the test.
pub fn schemas() -> Vec<TableSchema> {
    vec![
        RpxAuthor::schema(),
        RpxBlogPost::schema(),
        RpxTag::schema(),
        RpxCategory::schema(),
        RpxNodeA::schema(),
        RpxNodeB::schema(),
        RpxNote::schema(),
        RpxCounter::schema(),
        RpxEvent::schema(),
        RpxPin::schema(),
        RpxLegacy::schema(),
        RpxPathTyped::schema(),
        RpxSink::schema(),
    ]
}
