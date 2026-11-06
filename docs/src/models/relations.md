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

The column is stored as `UUID` (Postgres) or `TEXT` (SQLite). Every FK column is indexed.

### On delete

The database enforces the FK with a real `REFERENCES … ON DELETE …` clause, so deleting a parent costs one statement, with no per-row queries in Rust. Pick the action with `#[field(on_delete = "...")]`:

| Value | SQL | Deleting the parent… |
|---|---|---|
| `"cascade"` (default) | `ON DELETE CASCADE` | deletes the children |
| `"set_null"` | `ON DELETE SET NULL` | sets the children's FK to `NULL`; the field must be `Option<ForeignKey<M>>` (compile error otherwise) |
| `"set_default"` | `ON DELETE SET DEFAULT` | sets the children's FK to the column default |
| `"restrict"` | `ON DELETE RESTRICT` | fails with `Error::ForeignKeyViolation` while children exist |
| `"no_action"` | `ON DELETE NO ACTION` | like `restrict`, checked at the end of the statement |

```rust
#[derive(Model)]
pub struct Comment {
    pub id: FieldUuid,
    #[field(on_delete = "set_null")]
    pub author_id: Option<ForeignKey<User>>,
    #[field(on_delete = "restrict")]
    pub post_id: ForeignKey<Post>,
}
```

Changing `on_delete` produces a migration. `ON UPDATE` is always `NO ACTION`.

### Self-references and cycles

A model can reference itself (`parent_id: Option<ForeignKey<Category>>`), and two models can reference each other. `rango makemigrations` creates the tables first and adds the FKs that point forward with `ALTER TABLE … ADD CONSTRAINT` afterwards (inline on SQLite). Make at least one FK of a cycle nullable, otherwise no row can be inserted first.

### Querying with select_related

Load the related model in a single JOIN:

```rust
let results = Post::filter(&pool)
    .select_related(Post::AUTHOR_ID)
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

This creates a pivot table `article_tag` with two UUID columns.

### M2M operations

`M2M` works on every backend. `add` is idempotent (adding an existing link is a no-op).

```rust
// Add a relation
M2M::<Article, Tag>::add(&pool, &article, &tag).await?;

// Remove a relation
M2M::<Article, Tag>::remove(&pool, &article, &tag).await?;

// Get all tags for an article
let tags = M2M::<Article, Tag>::all(&pool, &article).await?;

// Get all articles for a tag (reverse)
let articles = M2M::<Article, Tag>::reverse(&pool, &tag).await?;

// Set exact relations (replaces all existing, in one transaction)
M2M::<Article, Tag>::set(&pool, &article, &tags).await?;

// Clear all relations
M2M::<Article, Tag>::clear(&pool, &article).await?;

// Check if relation exists
let exists = M2M::<Article, Tag>::exists(&pool, &article, &tag).await?;
```

### Self-referencing ManyToMany

A model can point a `ManyToMany` at itself:

```rust
#[derive(Model)]
pub struct User {
    pub id: FieldUuid,
    pub name: FieldText,
    pub followers: ManyToMany<User>,
}
```

The pivot table is `user_user`. Its two columns would clash if both were named `user_id`, so
they are `from_user_id` and `to_user_id` (only when both sides are the same table).

The relation is directional: `M2M::<User, User>::add(&pool, &a, &b)` stores `a` as `from` and
`b` as `to`. `all(&a)` returns the users `a` points to (`b`), and `reverse(&b)` returns the
users that point to `b` (`a`). Adding `(b, a)` as well makes the relation symmetric; Rango
never does that for you. `prefetch_related(User::FOLLOWERS)` follows the same direction as `all`.

## prefetch_related

Load reverse foreign key relations in one batched query (no N+1):

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

Rango executes:
1. `SELECT * FROM posts`
2. `SELECT * FROM comments WHERE post_id IN (...)`

Then merges in memory — no N+1, no magic.

`ManyToMany` fields work the same way: `.prefetch_related(Article::TAGS)` loads every article's tags in one query through the pivot table, then `article.prefetched::<Tag>()`.

## Nullable FK

```rust
pub author_id: Option<ForeignKey<User>>,  // nullable FK
```
