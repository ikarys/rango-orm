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

For direct foreign keys — executes a single JOIN:

```rust
let posts = Post::filter(&pool)
    .select_related::<User>("author_id")
    .all()
    .await?;

for post in &posts {
    let author = post.related::<User>(); // &User, zero SQL
    println!("{} by {}", post.title.0, author.name.0);
}
```

SQL generated:
```sql
SELECT t1.*, t2."id" AS "t2_id", t2."name" AS "t2_name", ...
FROM "myapp_post" t1
INNER JOIN "myapp_user" t2 ON t1."author_id" = t2."id"
```

## prefetch_related (reverse FK → batch IN)

For reverse FK or M2M — executes a second batched query:

```rust
let posts = Post::filter(&pool)
    .prefetch_related::<Comment>("post_id")
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
SELECT * FROM "myapp_comment" WHERE "post_id" IN ($1, $2, $3, ...)
```

## Combining both

```rust
let posts = Post::filter(&pool)
    .select_related::<User>("author_id")      // FK → JOIN
    .prefetch_related::<Comment>("post_id")   // reverse → batch IN
    .prefetch_related::<Tag>("post_id")       // M2M → batch IN
    .all()
    .await?;

for post in &posts {
    let author   = post.related::<User>();
    let comments = post.prefetched::<Comment>();
    let tags     = post.prefetched::<Tag>();
}
```

## Panics

Calling `.related::<R>()` or `.prefetched::<R>()` without the corresponding
`select_related` / `prefetch_related` call panics with a clear message:

```
thread panicked: select_related<User> was not loaded;
add .select_related::<User>("col") to the QueryBuilder chain
```

## Large result sets

`prefetch_related` should only be used with paginated results.
Loading 10 000 posts and prefetching all their comments will use significant memory.
Always combine with `.limit()`.
