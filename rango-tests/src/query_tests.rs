use rango_core::*;
use rango_derive::Model;
use rango_postgres::{PgPool, RangoFilterExt};

/// Minimal model used only for query-builder SQL generation tests.
#[derive(Model)]
struct TestQb {
    id: FieldUuid,
    active: FieldBool,
    role: FieldText,
    age: FieldInt,
    deleted: FieldBool,
    a: FieldInt,
    b: FieldInt,
    name: FieldText,
    created_at: FieldDateTime,
}

/// Lazy pool — no real connection, just needed to instantiate QueryBuilder.
fn pool() -> PgPool {
    PgPool::connect_lazy("postgres://test:test@localhost/test")
        .expect("connect_lazy should not fail with a valid URL")
}

// ── Basic filter operators ────────────────────────────────────────────────────

#[tokio::test]
async fn test_eq_generates_where() {
    let sql = TestQb::filter(&pool()).eq("active", true).explain();
    assert!(sql.contains("WHERE \"active\" = $1"), "got: {}", sql);
}

#[tokio::test]
async fn test_ne_generates_where() {
    let sql = TestQb::filter(&pool()).ne("role", "admin").explain();
    assert!(sql.contains("WHERE \"role\" != $1"), "got: {}", sql);
}

#[tokio::test]
async fn test_gt_generates_where() {
    let sql = TestQb::filter(&pool()).gt("age", 18i32).explain();
    assert!(sql.contains("WHERE \"age\" > $1"), "got: {}", sql);
}

// ── OR connector ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_or_connector() {
    let sql = TestQb::filter(&pool())
        .eq("role", "admin")
        .or()
        .eq("role", "mod")
        .explain();
    assert!(sql.contains("WHERE \"role\" = $1 OR \"role\" = $2"), "got: {}", sql);
}

// ── NOT modifier ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_not_modifier() {
    let sql = TestQb::filter(&pool()).not().eq("deleted", true).explain();
    assert!(sql.contains("WHERE NOT \"deleted\" = $1"), "got: {}", sql);
}

// ── Group (parenthesised OR) ──────────────────────────────────────────────────

#[tokio::test]
async fn test_group_parentheses() {
    let sql = TestQb::filter(&pool())
        .group(|q| q.eq("a", 1i32).or().eq("b", 2i32))
        .explain();
    assert!(sql.contains("WHERE (\"a\" = $1 OR \"b\" = $2)"), "got: {}", sql);
}

// ── ORDER BY ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_order_by_asc() {
    let sql = TestQb::filter(&pool()).order_by("name").explain();
    assert!(sql.contains("ORDER BY \"name\" ASC"), "got: {}", sql);
}

#[tokio::test]
async fn test_order_by_desc() {
    let sql = TestQb::filter(&pool()).order_by("-created_at").explain();
    assert!(sql.contains("ORDER BY \"created_at\" DESC"), "got: {}", sql);
}

// ── LIMIT / OFFSET ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_default_limit() {
    let sql = TestQb::filter(&pool()).explain();
    assert!(sql.contains("LIMIT 1000"), "default cap should be LIMIT 1000; got: {}", sql);
}

#[tokio::test]
async fn test_explicit_limit() {
    let sql = TestQb::filter(&pool()).limit(50).explain();
    assert!(sql.contains("LIMIT 50"), "got: {}", sql);
    assert!(!sql.contains("LIMIT 1000"), "should not have default cap; got: {}", sql);
}

#[tokio::test]
async fn test_unlimited_no_limit_clause() {
    let sql = TestQb::filter(&pool()).unlimited().explain();
    assert!(!sql.contains("LIMIT"), "unlimited() should remove LIMIT clause; got: {}", sql);
}

#[tokio::test]
async fn test_offset() {
    let sql = TestQb::filter(&pool()).offset(20).explain();
    assert!(sql.contains("OFFSET 20"), "got: {}", sql);
}

// ── No conditions → no WHERE ──────────────────────────────────────────────────

#[tokio::test]
async fn test_no_filter_no_where() {
    let sql = TestQb::filter(&pool()).explain();
    assert!(!sql.contains("WHERE"), "no filter should produce no WHERE; got: {}", sql);
}

// ── Full SQL structure ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_explain_starts_with_select() {
    let sql = TestQb::filter(&pool()).explain();
    assert!(sql.starts_with("SELECT * FROM \"test_qb\""), "got: {}", sql);
}
