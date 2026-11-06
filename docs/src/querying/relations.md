# select_related & prefetch_related

## The N+1 problem

Without relation loading, fetching posts and their authors requires N+1 queries:

```rust
// ❌ N+1 — 1 query for posts + N queries for authors
let posts = Post::filter(&pool).all().await?;
for post in &posts {
    let author = get::<_, User>(&pool, &post.author_id.to_sql_value()).await?;
    // ^ one query per post!
}
```

Rango solves this with `select_related` and `prefetch_related`.

## select_related (FK → JOIN)

For direct foreign keys: one query, with a `LEFT JOIN` per related model:

```rust
let posts = Post::filter(&pool)
    .select_related(Post::AUTHOR_ID)
    .all()
    .await?;

for post in &posts {
    let author = post.related::<User>(); // &User, zero SQL
    println!("{} by {}", post.title.0, author.name.0);
}
```

SQL generated:
```sql
SELECT "__m".*, "__r0"."id" AS "__r0_id", "__r0"."name" AS "__r0_name", ...
FROM (SELECT * FROM "myapp_post" WHERE ... ORDER BY ... LIMIT 1000) AS "__m"
LEFT JOIN "myapp_user" AS "__r0" ON "__r0"."id" = "__m"."author_id"
```

Filters, ordering and `LIMIT` apply to the posts before the join. A post whose FK is `NULL` comes back without a related object: `.related::<User>()` panics on it, so only use `select_related` on nullable FKs when you know they are set.

## prefetch_related (reverse FK → one batched query)

For reverse foreign keys (`Comment.post_id → Post`), one second batched query:

```rust
let posts = Post::filter(&pool)
    .prefetch_related(Comment::POST_ID)
    .all()
    .await?;

for post in &posts {
    let comments = post.prefetched::<Comment>(); // &[Comment], zero SQL
    println!("{} has {} comments", post.title.0, comments.len());
}
```

SQL generated:
```sql
-- Query 1
SELECT * FROM "myapp_post"

-- Query 2
SELECT * FROM "myapp_comment" WHERE "post_id" = ANY($1)  -- $1: array of post ids
```

SQLite and MySQL have no array parameters: they send `WHERE "post_id" IN (?, ?, ...)` in batches of 900 keys. Every backend runs the main query and the prefetch queries on one connection, and `all()` returns `Vec<WithRelated<M>>` everywhere (it derefs to `M`, so `post.title` works as before).

## Combining both

```rust
let posts = Post::filter(&pool)
    .select_related(Post::AUTHOR_ID)      // FK → JOIN
    .prefetch_related(Comment::POST_ID)   // reverse → batch IN
    .all()
    .await?;

for post in &posts {
    let author   = post.related::<User>();
    let comments = post.prefetched::<Comment>();
}
```

## Panics

Calling `.related::<R>()` or `.prefetched::<R>()` without the corresponding
`select_related` / `prefetch_related` call panics with a clear message:

```
thread panicked: select_related<User> was not loaded;
add .select_related(..) for User to the QueryBuilder chain
```

## Large result sets

Both queries run on the same pooled connection. The keys are bound as one array parameter, so the statement is the same for 1 or 50 000 posts and is prepared once.

`prefetch_related` should only be used with paginated results.
Loading 10 000 posts and prefetching all their comments will use significant memory.
Always combine with `.limit()`.
