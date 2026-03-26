# Relations

## ForeignKey

A `ForeignKey<M>` stores the UUID of a related model.

```rust
use rango::prelude::*;

#[derive(Model)]
pub struct User {
    pub id: FieldUuid,
    pub name: FieldVarchar<1, 100>,
}

#[derive(Model)]
pub struct Post {
    pub id: FieldUuid,
    pub author_id: ForeignKey<User>,  // → UUID NOT NULL
    pub title: FieldVarchar<1, 255>,
}
```

The column is stored as `UUID` (Postgres) or `TEXT` (SQLite).

### Querying with select_related

Load the related model in a single JOIN:

```rust
let results = Post::filter(&pool)
    .select_related::<User>("author_id")
    .all()
    .await?;

for post in &results {
    let author = post.related::<User>();
    println!("{} by {}", post.title.0, author.name.0);
}
```

## ManyToMany

Declare a `ManyToMany<M>` field — Rango auto-generates the pivot table.

```rust
#[derive(Model)]
pub struct Article {
    pub id: FieldUuid,
    pub title: FieldVarchar<1, 255>,
    pub tags: ManyToMany<Tag>,
}

#[derive(Model)]
pub struct Tag {
    pub id: FieldUuid,
    pub name: FieldVarchar<1, 50>,
}
```

This creates a pivot table `myapp_article_myapp_tag` with two UUID columns.

### M2M operations

```rust
// Add a relation
M2M::<Article, Tag>::add(&pool, &article, &tag).await?;

// Remove a relation
M2M::<Article, Tag>::remove(&pool, &article, &tag).await?;

// Get all tags for an article
let tags = M2M::<Article, Tag>::all(&pool, &article).await?;

// Get all articles for a tag (reverse)
let articles = M2M::<Article, Tag>::reverse(&pool, &tag).await?;

// Set exact relations (replaces all existing)
M2M::<Article, Tag>::set(&pool, &article, &tags).await?;

// Clear all relations
M2M::<Article, Tag>::clear(&pool, &article).await?;

// Check if relation exists
let exists = M2M::<Article, Tag>::exists(&pool, &article, &tag).await?;
```

## prefetch_related

Load reverse FK or M2M relations in a single batched query (no N+1):

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

Rango executes:
1. `SELECT * FROM posts`
2. `SELECT * FROM comments WHERE post_id IN (...)`

Then merges in memory — no N+1, no magic.

## Nullable FK

```rust
pub author_id: Option<ForeignKey<User>>,  // nullable FK
```
