use super::Bencher;
use super::consts;
use std::collections::HashMap;
use tokio::runtime::Runtime;

use rango::prelude::*;

#[derive(Model)]
#[model(table = "users")]
pub struct User {
    pub id: FieldAutoInt,
    pub name: FieldText,
    pub hair_color: Option<FieldText>,
}

#[derive(Model)]
#[model(table = "posts")]
pub struct Post {
    pub id: FieldAutoInt,
    pub user_id: FieldInt,
    pub title: FieldText,
    pub body: Option<FieldText>,
}

#[derive(Model)]
#[model(table = "comments")]
pub struct Comment {
    pub id: FieldAutoInt,
    pub post_id: FieldInt,
    pub text: FieldText,
}

pub fn connection() -> (Runtime, PgPool) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create runtime");
    dotenvy::dotenv().ok();
    let connection_url = dotenvy::var("POSTGRES_DATABASE_URL")
        .or_else(|_| dotenvy::var("DATABASE_URL"))
        .expect("DATABASE_URL must be set in order to run tests");

    let pool: PgPool = runtime.block_on(async {
        let mut cfg = DatabaseConfig::from_url(&connection_url);
        cfg.max_connections = 1;
        let pool: PgPool = rango::connect_postgres(&cfg).await.unwrap();
        for query in consts::postgres::CLEANUP_QUERIES {
            rango::raw_execute(&pool, query, vec![]).await.unwrap();
        }
        pool
    });

    (runtime, pool)
}

pub async fn insert_users(
    size: usize,
    pool: &PgPool,
    hair_color_init: impl Fn(usize) -> Option<&'static str>,
) {
    if size == 0 {
        return;
    }

    let users: Vec<User> = consts::build_insert_users_params(size, hair_color_init)
        .into_iter()
        .map(|(name, hair_color)| User {
            id: FieldAutoInt(None),
            name: FieldText(name),
            hair_color: hair_color.map(|hc| FieldText(hc.to_string())),
        })
        .collect();

    rango::bulk_create(pool, &users).await.unwrap();
}

pub fn bench_trivial_query(b: &mut Bencher, size: usize) {
    let (rt, pool) = connection();
    rt.block_on(insert_users(size, &pool, |_| None));

    b.iter(|| rt.block_on(async { User::filter(&pool).unlimited().all().await.unwrap() }));
}

pub fn bench_medium_complex_query(b: &mut Bencher, size: usize) {
    let (rt, pool) = connection();
    rt.block_on(insert_users(size, &pool, |i| {
        Some(if i % 2 == 0 { "black" } else { "brown" })
    }));

    // Rango has no JOIN API, so this is 2 queries (users, then posts IN (...)) instead of one LEFT JOIN.
    b.iter(|| {
        rt.block_on(async {
            User::filter(&pool)
                .eq("hair_color", "black")
                .unlimited()
                .prefetch_related::<Post>("user_id")
                .all()
                .await
                .unwrap()
        })
    });
}

pub fn bench_insert(b: &mut Bencher, size: usize) {
    let (rt, pool) = connection();

    b.iter(|| rt.block_on(insert_users(size, &pool, |_| Some("hair_color"))));
}

pub fn loading_associations_sequentially(b: &mut Bencher) {
    let (rt, pool) = connection();

    rt.block_on(async {
        insert_users(100, &pool, |i| {
            Some(if i % 2 == 0 { "black" } else { "brown" })
        })
        .await;

        let users = User::filter(&pool).unlimited().all().await.unwrap();

        let mut posts = Vec::with_capacity(users.len() * 10);
        for user in &users {
            let user_id = user.id.0.unwrap();
            for i in 0..10 {
                posts.push(Post {
                    id: FieldAutoInt(None),
                    user_id: FieldInt(user_id),
                    title: FieldText(format!("Post {} by user {}", i, user_id)),
                    body: None,
                });
            }
        }
        rango::bulk_create(&pool, &posts).await.unwrap();

        let posts = Post::filter(&pool).unlimited().all().await.unwrap();

        let mut comments = Vec::with_capacity(posts.len() * 10);
        for post in &posts {
            let post_id = post.id.0.unwrap();
            for i in 0..10 {
                comments.push(Comment {
                    id: FieldAutoInt(None),
                    post_id: FieldInt(post_id),
                    text: FieldText(format!("Comment {} on post {}", i, post_id)),
                });
            }
        }
        rango::bulk_create(&pool, &comments).await.unwrap();
    });

    b.iter(|| {
        rt.block_on(async {
            let users = User::filter(&pool)
                .unlimited()
                .prefetch_related::<Post>("user_id")
                .all()
                .await
                .unwrap();

            let mut ids = Vec::new();
            for user in &users {
                for post in user.prefetched::<Post>() {
                    ids.push(SqlValue::Int(post.id.0.unwrap()));
                }
            }

            let comments = Comment::filter(&pool)
                .in_values("post_id", ids)
                .unlimited()
                .all()
                .await
                .unwrap();

            let mut by_post: HashMap<i32, Vec<Comment>> = HashMap::new();
            for comment in comments {
                let comment = comment.inner;
                by_post.entry(comment.post_id.0).or_default().push(comment);
            }

            let mut result = Vec::with_capacity(users.len());
            for user in users {
                let user_id = user.id.0.unwrap();
                let mut user_posts = Vec::new();

                for post in user.prefetched::<Post>() {
                    let post_id = post.id.0.unwrap();
                    let post_comments = by_post.remove(&post_id).unwrap_or_default();
                    user_posts.push((post_id, post_comments));
                }

                result.push((user_id, user_posts));
            }

            result
        })
    });
}
