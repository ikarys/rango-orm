//! The queries every backend must answer the same way, run against the migrated `rpx_*`
//! tables. Expanded once per backend crate: `scenario!(rango_orm_sqlite, SqlitePool)`.

#[macro_export]
macro_rules! scenario {
    ($be:ident, $Pool:ty) => {
        mod scenario {
            use super::models::*;
            use $be::*;
            use rango::prelude::{
                FieldAutoBigInt, FieldBigInt, FieldBool, FieldBytes, FieldDate, FieldDateTime,
                FieldDecimal, FieldDouble, FieldEmail, FieldFloat, FieldInt, FieldJson,
                FieldPassword, FieldRange, FieldSmallInt, FieldText, FieldTime, FieldUrl,
                FieldUuid, FieldVarchar, ForeignKey, ManyToMany, ToSqlValue,
            };
            use uuid::Uuid;

            fn author(n: &str, karma: Option<i32>) -> RpxAuthor {
                RpxAuthor {
                    id: FieldUuid(Uuid::new_v4()),
                    email: FieldEmail(format!("{n}@rpx.test")),
                    name: FieldVarchar(n.to_string()),
                    bio: None,
                    karma: karma.map(FieldInt),
                    created: FieldDateTime::now(),
                }
            }

            fn post(a: &RpxAuthor, title: &str, rating: i32) -> RpxBlogPost {
                RpxBlogPost {
                    id: FieldUuid(Uuid::new_v4()),
                    author: ForeignKey::new(a.id.0),
                    reviewer: None,
                    title: FieldVarchar(title.to_string()),
                    body: FieldText(format!("body of {title}")),
                    published: FieldBool(true),
                    rating: FieldInt(rating),
                    tags: ManyToMany::empty(),
                }
            }

            fn tag(n: &str) -> RpxTag {
                RpxTag {
                    id: FieldUuid(Uuid::new_v4()),
                    name: FieldVarchar(n.to_string()),
                }
            }

            pub async fn run(pool: &$Pool) {
                crud_filter_order(pool).await;
                relations(pool).await;
                m2m(pool).await;
                partial_bulk_keyset(pool).await;
                fk_cycle(pool).await;
                implicit_pk_and_sink(pool).await;
            }

            async fn crud_filter_order(pool: &$Pool) {
                let ada = insert(pool, author("ada", Some(10))).await.unwrap();
                let bob = insert(pool, author("bob", Some(2))).await.unwrap();
                insert(pool, author("cyd", None)).await.unwrap();

                let got = get::<_, RpxAuthor>(pool, &ada.id.to_sql_value())
                    .await
                    .unwrap()
.unwrap();
                assert_eq!(got.email, ada.email);
                assert_eq!(got.karma, Some(FieldInt(10)));

                let mut upd = bob.clone();
                upd.karma = Some(FieldInt(3));
                upd.bio = Some(FieldText("hello".into()));
                update(pool, upd).await.unwrap();
                let got = get::<_, RpxAuthor>(pool, &bob.id.to_sql_value())
                    .await
                    .unwrap()
.unwrap();
                assert_eq!(got.karma, Some(FieldInt(3)));
                assert_eq!(got.bio, Some(FieldText("hello".into())));

                // Default ordering is `ordering = [asc("name")]`.
                let all = RpxAuthor::filter(pool).all().await.unwrap();
                let names: Vec<_> = all.iter().map(|a| a.name.0.clone()).collect();
                assert_eq!(names, ["ada", "bob", "cyd"]);

                let top = RpxAuthor::filter(pool)
                    .gte(RpxAuthor::KARMA, 3)
                    .order_by("-name")
                    .all()
                    .await
                    .unwrap();
                let names: Vec<_> = top.iter().map(|a| a.name.0.clone()).collect();
                assert_eq!(names, ["bob", "ada"]);

                let victim = author("victim", None);
                insert(pool, victim.clone()).await.unwrap();
                delete(pool, &victim).await.unwrap();
                assert!(
                    RpxAuthor::filter(pool)
                        .eq(RpxAuthor::NAME, "victim")
                        .one()
                        .await
                        .unwrap()
                        .is_none()
                );
            }

            async fn relations(pool: &$Pool) {
                let ada = RpxAuthor::filter(pool)
                    .eq(RpxAuthor::NAME, "ada")
                    .one()
                    .await
                    .unwrap()
                    .unwrap();
                let bob = RpxAuthor::filter(pool)
                    .eq(RpxAuthor::NAME, "bob")
                    .one()
                    .await
                    .unwrap()
                    .unwrap();

                let mut p1 = post(&ada, "first", 5);
                p1.reviewer = Some(ForeignKey::new(bob.id.0));
                insert(pool, p1.clone()).await.unwrap();
                insert(pool, post(&ada, "second", 1)).await.unwrap();
                insert(pool, post(&bob, "third", 0)).await.unwrap();

                // Constraints declared on the model are enforced by the database.
                assert!(
                    insert(pool, post(&ada, "bad", -1)).await.is_err(),
                    "CHECK rating >= 0"
                );
                assert!(
                    insert(pool, post(&ada, "first", 2)).await.is_err(),
                    "UNIQUE (author, title)"
                );

                let posts = RpxBlogPost::filter(pool)
                    .select_related(RpxBlogPost::AUTHOR)
                    .order_by("title")
                    .all()
                    .await
                    .unwrap();
                assert_eq!(posts.len(), 3);
                assert_eq!(posts[0].title.0, "first");
                assert_eq!(posts[0].related::<RpxAuthor>().name.0, "ada");
                assert_eq!(posts[0].body.0, "body of first");

                let authors = RpxAuthor::filter(pool)
                    .prefetch_related(RpxBlogPost::AUTHOR)
                    .all()
                    .await
                    .unwrap();
                let n = |name: &str| {
                    authors
                        .iter()
                        .find(|a| a.name.0 == name)
                        .unwrap()
                        .prefetched::<RpxBlogPost>()
                        .len()
                };
                assert_eq!((n("ada"), n("bob"), n("cyd")), (2, 1, 0));
            }

            async fn m2m(pool: &$Pool) {
                let by_title = |t: &'static str| async move {
                    RpxBlogPost::filter(pool)
                        .eq(RpxBlogPost::TITLE, t)
                        .one()
                        .await
                        .unwrap()
                        .unwrap()
                };
                let first = by_title("first").await;
                let second = by_title("second").await;
                let (rust, orm) = (tag("rust"), tag("orm"));
                insert(pool, rust.clone()).await.unwrap();
                insert(pool, orm.clone()).await.unwrap();
                M2M::<RpxBlogPost, RpxTag>::add(pool, &first, &rust)
                    .await
                    .unwrap();
                M2M::<RpxBlogPost, RpxTag>::add(pool, &first, &orm)
                    .await
                    .unwrap();
                M2M::<RpxBlogPost, RpxTag>::add(pool, &second, &rust)
                    .await
                    .unwrap();

                let mut names: Vec<_> = M2M::<RpxBlogPost, RpxTag>::all(pool, &first)
                    .await
                    .unwrap()
                    .iter()
                    .map(|t| t.name.0.clone())
                    .collect();
                names.sort();
                assert_eq!(names, ["orm", "rust"]);

                let posts = RpxBlogPost::filter(pool)
                    .prefetch_related(RpxBlogPost::TAGS)
                    .all()
                    .await
                    .unwrap();
                let count = |t: &str| {
                    posts
                        .iter()
                        .find(|p| p.title.0 == t)
                        .unwrap()
                        .prefetched::<RpxTag>()
                        .len()
                };
                assert_eq!((count("first"), count("second"), count("third")), (2, 1, 0));
            }

            async fn partial_bulk_keyset(pool: &$Pool) {
                let lite = RpxAuthor::filter(pool)
                    .select_as::<RpxAuthorLite>()
                    .all()
                    .await
                    .unwrap();
                assert!(lite.iter().any(|l| l.name.0 == "ada" && l.bio.is_none()));
                assert!(lite.iter().any(|l| l.name.0 == "bob" && l.bio.is_some()));

                let events: Vec<RpxEvent> = (0..7)
                    .map(|i| RpxEvent {
                        id: FieldAutoBigInt(None),
                        kind: FieldText(format!("k{i}")),
                    })
                    .collect();
                assert_eq!(bulk_create(pool, &events).await.unwrap(), 7);

                // Keyset pagination walks every row exactly once, in id order.
                let mut seen = Vec::new();
                let mut last: Option<i64> = None;
                loop {
                    let mut q = RpxEvent::filter(pool).limit(3);
                    if let Some(id) = last {
                        q = q.after(RpxEvent::ID, id);
                    }
                    let page = q.order_by("id").all().await.unwrap();
                    let Some(row) = page.last() else { break };
                    last = row.id.0;
                    seen.extend(page.iter().map(|e| e.kind.0.clone()));
                }
                assert_eq!(seen, ["k0", "k1", "k2", "k3", "k4", "k5", "k6"]);
            }

            async fn fk_cycle(pool: &$Pool) {
                let mut a = RpxNodeA {
                    id: FieldUuid(Uuid::new_v4()),
                    label: FieldText("a".into()),
                    b: None,
                };
                insert(pool, a.clone()).await.unwrap();
                let b = RpxNodeB {
                    id: FieldUuid(Uuid::new_v4()),
                    label: FieldText("b".into()),
                    a: Some(ForeignKey::new(a.id.0)),
                };
                insert(pool, b.clone()).await.unwrap();
                a.b = Some(ForeignKey::new(b.id.0));
                update(pool, a.clone()).await.unwrap();
                let got = get::<_, RpxNodeA>(pool, &a.id.to_sql_value())
                    .await
                    .unwrap()
.unwrap();
                assert_eq!(got.b.map(|f| f.0), Some(b.id.0));

                // Self reference.
                let root = RpxCategory {
                    id: FieldUuid(Uuid::new_v4()),
                    name: FieldText("root".into()),
                    parent: None,
                };
                insert(pool, root.clone()).await.unwrap();
                let child = RpxCategory {
                    id: FieldUuid(Uuid::new_v4()),
                    name: FieldText("child".into()),
                    parent: Some(ForeignKey::new(root.id.0)),
                };
                insert(pool, child.clone()).await.unwrap();
                let got = get::<_, RpxCategory>(pool, &child.id.to_sql_value())
                    .await
                    .unwrap()
.unwrap();
                assert_eq!(got.parent.map(|f| f.0), Some(root.id.0));
            }

            async fn implicit_pk_and_sink(pool: &$Pool) {
                let note = insert(
                    pool,
                    RpxNote {
                        id: FieldUuid(Uuid::new_v4()),
                        title: FieldText("n".into()),
                    },
                )
                .await
                .unwrap();
                let got = get::<_, RpxNote>(pool, &note.id.to_sql_value())
                    .await
                    .unwrap()
.unwrap();
                assert_eq!(got.title.0, "n");

                let c = insert(
                    pool,
                    RpxCounter {
                        id: FieldAutoBigInt(None),
                        name: FieldVarchar("c".into()),
                        hits: FieldBigInt(1),
                    },
                )
                .await
                .unwrap();
                assert!(c.id.0.is_some(), "database assigns the id");

                let ada = RpxAuthor::filter(pool)
                    .eq(RpxAuthor::NAME, "ada")
                    .one()
                    .await
                    .unwrap()
                    .unwrap();
                insert(
                    pool,
                    RpxPathTyped {
                        id: rango::FieldUuid(Uuid::new_v4()),
                        label: rango::FieldText("p".into()),
                        count: None,
                        author: rango::ForeignKey::new(ada.id.0),
                    },
                )
                .await
                .unwrap();

                let sink = RpxSink {
                    id: FieldUuid(Uuid::new_v4()),
                    b: FieldBool(true),
                    small: FieldSmallInt(7),
                    int: FieldInt(-5),
                    big: FieldBigInt(1 << 40),
                    float: FieldFloat(1.5),
                    double: FieldDouble(2.25),
                    dec: FieldDecimal(12.5),
                    text: FieldText("t".into()),
                    short: FieldVarchar("s".into()),
                    email: FieldEmail("s@rpx.test".into()),
                    url: FieldUrl("https://rpx.test/x".into()),
                    bytes: FieldBytes(vec![0, 1, 2, 255]),
                    day: FieldDate(chrono::NaiveDate::from_ymd_opt(2026, 11, 6).unwrap()),
                    at: FieldTime(chrono::NaiveTime::from_hms_opt(12, 30, 15).unwrap()),
                    stamp: FieldDateTime::now(),
                    json: FieldJson(serde_json::json!({"k": [1, 2]})),
                    range: FieldRange(42),
                    secret: FieldPassword::new("hunter2hunter2").unwrap(),
                };
                insert(pool, sink.clone()).await.unwrap();
                let got = get::<_, RpxSink>(pool, &sink.id.to_sql_value())
                    .await
                    .unwrap()
.unwrap();
                assert_eq!(got.small, sink.small);
                assert_eq!(got.big, sink.big);
                assert_eq!(got.dec, sink.dec);
                assert_eq!(got.bytes, sink.bytes);
                assert_eq!(got.day, sink.day);
                assert_eq!(got.at, sink.at);
                assert_eq!(got.json, sink.json);
                assert_eq!(got.range, sink.range);
                assert!(got.secret.verify("hunter2hunter2"));
            }
        }
    };
}
