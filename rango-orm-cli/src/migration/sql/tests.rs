use super::table::column_definition;
use super::{generate_sql_from_diff, object_name};
use crate::migration::diff::SchemaDiff;
use crate::migration::scanner::{M2MRelation, generate_pivot_table};
use crate::migration::snapshot::SnapshotConstraint;
use rango_orm_core::{
    BackendKind, ColumnDef, ColumnType, FkReference, ReferentialAction, TableSchema,
};

fn uuid_col(name: &str, pk: bool) -> ColumnDef {
    ColumnDef {
        name: name.to_string(),
        col_type: ColumnType::Uuid,
        nullable: false,
        primary_key: pk,
        unique: pk,
        default: None,
        references: None,
        index: false,
    }
}

fn text_col(name: &str) -> ColumnDef {
    ColumnDef {
        name: name.to_string(),
        col_type: ColumnType::Text,
        nullable: false,
        primary_key: false,
        unique: false,
        default: None,
        references: None,
        index: false,
    }
}

fn int_col(name: &str) -> ColumnDef {
    ColumnDef {
        name: name.to_string(),
        col_type: ColumnType::Int,
        nullable: false,
        primary_key: false,
        unique: false,
        default: None,
        references: None,
        index: false,
    }
}

fn datetime_col(name: &str) -> ColumnDef {
    ColumnDef {
        name: name.to_string(),
        col_type: ColumnType::DateTime,
        nullable: false,
        primary_key: false,
        unique: false,
        default: None,
        references: None,
        index: false,
    }
}

fn fk_col(name: &str, table: &str, on_delete: ReferentialAction) -> ColumnDef {
    ColumnDef {
        name: name.to_string(),
        col_type: ColumnType::Uuid,
        nullable: false,
        primary_key: false,
        unique: false,
        default: None,
        references: Some(FkReference {
            table: table.to_string(),
            column: "id".to_string(),
            on_delete,
            on_update: ReferentialAction::NoAction,
        }),
        index: true,
    }
}

fn simple_table(name: &str) -> TableSchema {
    TableSchema {
        table_name: name.to_string(),
        columns: vec![uuid_col("id", true), text_col("name")],
        ..Default::default()
    }
}

// ── CreateTable ────────────────────────────────────────────────────────────
#[test]
fn test_create_table_sql() {
    let diff = SchemaDiff::CreateTable(simple_table("users"));
    let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
    assert!(sql.contains("CREATE TABLE IF NOT EXISTS"), "got: {}", sql);
    assert!(sql.contains("\"users\""), "got: {}", sql);
}

#[test]
fn test_create_table_has_columns() {
    let diff = SchemaDiff::CreateTable(simple_table("orders"));
    let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
    assert!(sql.contains("\"id\""), "got: {}", sql);
    assert!(sql.contains("\"name\""), "got: {}", sql);
}

// ── AddColumn ─────────────────────────────────────────────────────────────
#[test]
fn test_add_column_sql() {
    let diff = SchemaDiff::AddColumn {
        table: "x".to_string(),
        column: text_col("title"),
    };
    let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
    assert!(sql.contains("ALTER TABLE \"x\" ADD COLUMN"), "got: {}", sql);
    assert!(sql.contains("\"title\""), "got: {}", sql);
}

// ── DropColumn ────────────────────────────────────────────────────────────
#[test]
fn test_drop_column_sql() {
    let diff = SchemaDiff::DropColumn {
        table: "x".to_string(),
        column: "old_col".to_string(),
    };
    let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
    assert!(
        sql.contains("ALTER TABLE \"x\" DROP COLUMN"),
        "got: {}",
        sql
    );
    assert!(sql.contains("\"old_col\""), "got: {}", sql);
}

// ── AlterColumnNullable ───────────────────────────────────────────────────
#[test]
fn test_alter_nullable_true() {
    let mut col = text_col("c");
    col.nullable = true;
    let diff = SchemaDiff::AlterColumnNullable {
        table: "t".to_string(),
        column: col,
    };
    let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
    assert!(sql.contains("DROP NOT NULL"), "got: {}", sql);
}

#[test]
fn test_alter_nullable_false() {
    let diff = SchemaDiff::AlterColumnNullable {
        table: "t".to_string(),
        column: text_col("c"),
    };
    let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
    assert!(sql.contains("SET NOT NULL"), "got: {}", sql);
}

// ── DropTable ─────────────────────────────────────────────────────────────
#[test]
fn test_drop_table_sql() {
    let diff = SchemaDiff::DropTable("old_table".to_string());
    let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
    assert!(sql.contains("DROP TABLE IF EXISTS"), "got: {}", sql);
    assert!(sql.contains("\"old_table\""), "got: {}", sql);
}

// ── Empty diff ────────────────────────────────────────────────────────────
#[test]
fn test_empty_diff_produces_empty_string() {
    let sql = generate_sql_from_diff(&[], rango_orm_core::BackendKind::Postgres);
    assert!(
        sql.is_empty(),
        "empty diff should produce empty string; got: {:?}",
        sql
    );
}

// ── foreign keys ──────────────────────────────────────────────────────────
#[test]
fn test_fk_column_sql_contains_references_cascade() {
    let schema = TableSchema {
        table_name: "posts".to_string(),
        columns: vec![
            uuid_col("id", true),
            fk_col("author", "app_users", ReferentialAction::Cascade),
        ],
        ..Default::default()
    };
    let sql = generate_sql_from_diff(
        &[SchemaDiff::CreateTable(schema)],
        rango_orm_core::BackendKind::Postgres,
    );
    assert!(
        sql.contains(
            "CONSTRAINT \"posts_author_fkey\" REFERENCES \"app_users\" (\"id\") ON DELETE CASCADE"
        ),
        "got: {}",
        sql
    );
}

#[test]
fn test_fk_column_sql_contains_references_set_null() {
    let mut col = fk_col("author", "app_users", ReferentialAction::SetNull);
    col.nullable = true;
    let schema = TableSchema {
        table_name: "posts".to_string(),
        columns: vec![uuid_col("id", true), col],
        ..Default::default()
    };
    let sql = generate_sql_from_diff(
        &[SchemaDiff::CreateTable(schema)],
        rango_orm_core::BackendKind::Postgres,
    );
    assert!(sql.contains("ON DELETE SET NULL"), "got: {}", sql);
}

// ── pivot tables ──────────────────────────────────────────────────────────
#[test]
fn test_pivot_table_sql_has_composite_pk_references_and_index() {
    let rel = M2MRelation {
        from_table: "posts".to_string(),
        to_table: "tags".to_string(),
    };
    let schema = generate_pivot_table(&rel, &[]);
    let sql = generate_sql_from_diff(
        &[SchemaDiff::CreateTable(schema)],
        rango_orm_core::BackendKind::Postgres,
    );

    assert!(
        sql.contains("PRIMARY KEY (\"posts_id\", \"tags_id\")"),
        "got: {}",
        sql
    );
    assert!(
        sql.contains("REFERENCES \"posts\" (\"id\") ON DELETE CASCADE"),
        "got: {}",
        sql
    );
    assert!(
        sql.contains("REFERENCES \"tags\" (\"id\") ON DELETE CASCADE"),
        "got: {}",
        sql
    );
    assert!(
        sql.contains(
            "CREATE INDEX IF NOT EXISTS \"posts_tags_tags_id_idx\" ON \"posts_tags\" (\"tags_id\");"
        ),
        "got: {}",
        sql
    );
}

// ── topological ordering ──────────────────────────────────────────────────
#[test]
fn test_create_tables_are_topologically_ordered() {
    let posts = TableSchema {
        table_name: "posts".to_string(),
        columns: vec![uuid_col("id", true), text_col("title")],
        ..Default::default()
    };
    let comments = TableSchema {
        table_name: "comments".to_string(),
        columns: vec![
            uuid_col("id", true),
            fk_col("post", "posts", ReferentialAction::Cascade),
        ],
        ..Default::default()
    };

    let diffs = vec![
        SchemaDiff::CreateTable(comments),
        SchemaDiff::CreateTable(posts),
    ];
    let sql = generate_sql_from_diff(&diffs, rango_orm_core::BackendKind::Postgres);

    let posts_pos = sql
        .find("CREATE TABLE IF NOT EXISTS \"posts\"")
        .expect("posts table should exist");
    let comments_pos = sql
        .find("CREATE TABLE IF NOT EXISTS \"comments\"")
        .expect("comments table should exist");

    assert!(posts_pos < comments_pos);
}

// ── circular foreign keys ─────────────────────────────────────────────────
fn cycle_diffs() -> Vec<SchemaDiff> {
    let table = |name: &str, other: &str| TableSchema {
        table_name: name.to_string(),
        columns: vec![
            uuid_col("id", true),
            fk_col(&format!("{other}_id"), other, ReferentialAction::Cascade),
        ],
        ..Default::default()
    };
    vec![
        SchemaDiff::CreateTable(table("projects", "generations")),
        SchemaDiff::CreateTable(table("generations", "projects")),
    ]
}

#[test]
fn test_circular_fk_deferred_to_alter_table() {
    for backend in [BackendKind::Postgres, BackendKind::Mysql] {
        let sql = generate_sql_from_diff(&cycle_diffs(), backend);
        let q = |n: &str| backend.quote_ident(n);
        let first = sql.find("CREATE TABLE").unwrap();
        let second = first + 1 + sql[first + 1..].find("CREATE TABLE").unwrap();
        let alter = sql.find("ALTER TABLE").expect(&sql);
        assert!(alter > second, "got: {sql}");
        // Only the second table keeps its inline reference (target already created).
        assert_eq!(sql.matches("REFERENCES").count(), 2, "got: {sql}");
        assert_eq!(sql[..alter].matches("REFERENCES").count(), 1, "got: {sql}");
        assert!(
            sql[alter..].contains(&format!(
                "ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE CASCADE{};",
                q("projects_generations_id_fkey"),
                q("generations_id"),
                q("generations"),
                q("id"),
                // MySQL has no deferrable constraints; restore turns the checks off.
                if backend == BackendKind::Postgres {
                    " DEFERRABLE INITIALLY IMMEDIATE"
                } else {
                    ""
                }
            )),
            "got: {sql}"
        );
    }
}

#[test]
fn test_circular_fk_stays_inline_on_sqlite() {
    let sql = generate_sql_from_diff(&cycle_diffs(), BackendKind::Sqlite);
    assert!(!sql.contains("ALTER TABLE"), "got: {sql}");
    assert_eq!(sql.matches("REFERENCES").count(), 2, "got: {sql}");
}

// ── indexes ───────────────────────────────────────────────────────────────
#[test]
fn test_indexed_column_emits_create_index() {
    let mut col = text_col("slug");
    col.index = true;
    let schema = TableSchema {
        table_name: "pages".to_string(),
        columns: vec![uuid_col("id", true), col],
        ..Default::default()
    };
    let sql = generate_sql_from_diff(
        &[SchemaDiff::CreateTable(schema)],
        rango_orm_core::BackendKind::Postgres,
    );
    assert!(
        sql.contains("CREATE INDEX IF NOT EXISTS \"pages_slug_idx\" ON \"pages\" (\"slug\");"),
        "got: {}",
        sql
    );
}

#[test]
fn test_fk_column_is_auto_indexed() {
    let schema = TableSchema {
        table_name: "posts".to_string(),
        columns: vec![
            uuid_col("id", true),
            fk_col("author", "app_users", ReferentialAction::Cascade),
        ],
        ..Default::default()
    };
    let sql = generate_sql_from_diff(
        &[SchemaDiff::CreateTable(schema)],
        rango_orm_core::BackendKind::Postgres,
    );
    assert!(
        sql.contains("CREATE INDEX IF NOT EXISTS \"posts_author_idx\" ON \"posts\" (\"author\");"),
        "got: {}",
        sql
    );
}

// ── rename + type change SQL ──────────────────────────────────────────────
#[test]
fn test_rename_and_type_change_sql() {
    let diffs = vec![
        SchemaDiff::RenameColumn {
            table: "t".to_string(),
            from: "amount".to_string(),
            to: "price".to_string(),
        },
        SchemaDiff::AlterColumnType {
            table: "t".to_string(),
            column: int_col("price"),
        },
    ];
    let sql = generate_sql_from_diff(&diffs, rango_orm_core::BackendKind::Postgres);

    let rename_pos = sql
        .find("ALTER TABLE \"t\" RENAME COLUMN \"amount\" TO \"price\";")
        .expect("rename should exist");
    let alter_pos = sql
        .find("ALTER TABLE \"t\" ALTER COLUMN \"price\" TYPE INTEGER USING \"price\"::INTEGER;")
        .expect("alter type should exist");

    assert!(rename_pos < alter_pos);
}

#[test]
fn test_pure_type_change_has_using_clause() {
    let mut col = int_col("value");
    col.col_type = ColumnType::BigInt;
    let diff = SchemaDiff::AlterColumnType {
        table: "t".to_string(),
        column: col,
    };
    let sql = generate_sql_from_diff(&[diff], rango_orm_core::BackendKind::Postgres);
    assert!(sql.contains("USING \"value\"::BIGINT"), "got: {}", sql);
}

// ── MySQL ─────────────────────────────────────────────────────────────────
#[test]
fn mysql_create_table_uses_backticks_engine_and_inline_index() {
    let mut col = text_col("slug");
    col.index = true;
    let schema = TableSchema {
        table_name: "pages".to_string(),
        columns: vec![uuid_col("id", true), datetime_col("created_at"), col],
        ..Default::default()
    };

    let sql = generate_sql_from_diff(&[SchemaDiff::CreateTable(schema)], BackendKind::Mysql);

    assert!(
        sql.contains("CREATE TABLE IF NOT EXISTS `pages`"),
        "got: {}",
        sql
    );
    assert!(sql.contains("BINARY(16)"), "got: {}", sql);
    assert!(sql.contains("DATETIME(6)"), "got: {}", sql);
    assert!(
        sql.contains("ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;"),
        "got: {}",
        sql
    );
    assert!(
        sql.contains("INDEX `pages_slug_idx` (`slug`(191))"),
        "got: {}",
        sql
    );
    assert!(!sql.contains("CREATE INDEX IF NOT EXISTS"), "got: {}", sql);
}

#[test]
fn mysql_create_table_uses_table_level_foreign_key() {
    let schema = TableSchema {
        table_name: "posts".to_string(),
        columns: vec![
            uuid_col("id", true),
            fk_col("author", "app_users", ReferentialAction::Cascade),
        ],
        ..Default::default()
    };

    let sql = generate_sql_from_diff(&[SchemaDiff::CreateTable(schema)], BackendKind::Mysql);

    assert!(
        sql.contains(
            "CONSTRAINT `posts_author_fkey` FOREIGN KEY (`author`) REFERENCES `app_users` (`id`) ON DELETE CASCADE"
        ),
        "got: {}",
        sql
    );
}

#[test]
fn mysql_alter_column_type_uses_modify_column() {
    let diff = SchemaDiff::AlterColumnType {
        table: "t".to_string(),
        column: int_col("c"),
    };

    let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

    assert!(
        sql.contains("ALTER TABLE `t` MODIFY COLUMN `c` INT NOT NULL;"),
        "got: {}",
        sql
    );
}

#[test]
fn mysql_alter_column_nullable_uses_modify_column() {
    let mut col = text_col("c");
    col.nullable = true;
    let diff = SchemaDiff::AlterColumnNullable {
        table: "t".to_string(),
        column: col,
    };

    let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

    assert!(
        sql.contains("ALTER TABLE `t` MODIFY COLUMN `c` LONGTEXT;"),
        "got: {}",
        sql
    );
}

#[test]
fn mysql_alter_column_reference_drops_foreign_key_only_when_had_reference() {
    let fk = FkReference {
        table: "users".to_string(),
        column: "id".to_string(),
        on_delete: ReferentialAction::Cascade,
        on_update: ReferentialAction::NoAction,
    };

    let with_old_fk = SchemaDiff::AlterColumnReference {
        table: "t".to_string(),
        column: "c".to_string(),
        references: Some(fk.clone()),
        had_reference: true,
    };
    let sql = generate_sql_from_diff(&[with_old_fk], BackendKind::Mysql);
    assert!(
        sql.contains("ALTER TABLE `t` DROP FOREIGN KEY `t_c_fkey`;"),
        "got: {}",
        sql
    );
    assert!(
        sql.contains(
            "ALTER TABLE `t` ADD CONSTRAINT `t_c_fkey` FOREIGN KEY (`c`) REFERENCES `users` (`id`) ON DELETE CASCADE;"
        ),
        "got: {}",
        sql
    );

    let without_old_fk = SchemaDiff::AlterColumnReference {
        table: "t".to_string(),
        column: "c".to_string(),
        references: Some(fk),
        had_reference: false,
    };
    let sql = generate_sql_from_diff(&[without_old_fk], BackendKind::Mysql);
    assert!(!sql.contains("DROP FOREIGN KEY"), "got: {}", sql);
    assert!(
        sql.contains(
            "ALTER TABLE `t` ADD CONSTRAINT `t_c_fkey` FOREIGN KEY (`c`) REFERENCES `users` (`id`) ON DELETE CASCADE;"
        ),
        "got: {}",
        sql
    );
}

#[test]
fn mysql_alter_column_index_uses_drop_index_on() {
    let diff = SchemaDiff::AlterColumnIndex {
        table: "t".to_string(),
        column: "c".to_string(),
        col_type: ColumnType::Uuid,
        index: false,
    };

    let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

    assert!(sql.contains("DROP INDEX `t_c_idx` ON `t`;"), "got: {}", sql);
    assert!(!sql.contains("DROP INDEX IF EXISTS"), "got: {}", sql);
}

#[test]
fn mysql_alter_column_index_on_text_and_blob_has_prefix_length() {
    let sql_for = |col_type, backend| {
        generate_sql_from_diff(
            &[SchemaDiff::AlterColumnIndex {
                table: "t".to_string(),
                column: "c".to_string(),
                col_type,
                index: true,
            }],
            backend,
        )
    };
    for ty in [ColumnType::Text, ColumnType::Bytea] {
        let sql = sql_for(ty, BackendKind::Mysql);
        assert!(sql.contains("ON `t` (`c`(191));"), "got: {}", sql);
    }
    assert!(sql_for(ColumnType::Uuid, BackendKind::Mysql).contains("ON `t` (`c`);"));
    assert!(sql_for(ColumnType::Text, BackendKind::Postgres).contains("ON \"t\" (\"c\");"));
}

#[test]
fn mysql_add_constraint_unique_requotes_columns() {
    let diff = SchemaDiff::AddConstraint {
        table: "t".to_string(),
        constraint: SnapshotConstraint {
            name: "t_room_date_unique".to_string(),
            definition: "UNIQUE (\"room\", \"date\")".to_string(),
        },
    };

    let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

    assert!(
        sql.contains(
            "ALTER TABLE `t` ADD CONSTRAINT `t_room_date_unique` UNIQUE (`room`, `date`);"
        ),
        "got: {}",
        sql
    );
}

#[test]
fn mysql_partial_unique_constraint_is_skipped_with_comment() {
    let diff = SchemaDiff::AddConstraint {
        table: "t".to_string(),
        constraint: SnapshotConstraint {
            name: "t_active_unique".to_string(),
            definition: "UNIQUE (\"email\") WHERE active".to_string(),
        },
    };

    let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

    assert!(
        sql.contains(
            "-- MySQL/MariaDB do not support partial unique indexes: constraint \"t_active_unique\" on \"t\" was skipped (enforce it in application code)."
        ),
        "got: {}",
        sql
    );
}

#[test]
fn mysql_non_empty_sql_has_header_comment() {
    let diff = SchemaDiff::DropTable("t".to_string());
    let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

    assert!(
        sql.starts_with(
            "-- MySQL/MariaDB: DDL statements are not transactional; a failing migration can leave earlier statements applied.\n\n"
        ),
        "got: {}",
        sql
    );
}

#[test]
fn mysql_identifier_with_backtick_is_doubled() {
    let diff = SchemaDiff::CreateTable(simple_table("we`ird"));
    let sql = generate_sql_from_diff(&[diff], BackendKind::Mysql);

    assert!(sql.contains("`we``ird`"), "got: {}", sql);
}

#[test]
fn mysql_long_object_names_are_shortened_with_a_stable_hash() {
    let long = format!("{}_fkey", "a".repeat(70));
    let quoted = object_name(BackendKind::Mysql, &long);
    let name = quoted.trim_matches('`');
    assert_eq!(name.len(), 64);
    assert!(name.starts_with(&"a".repeat(55)));
    assert_eq!(quoted, object_name(BackendKind::Mysql, &long));
    assert_ne!(
        quoted,
        object_name(BackendKind::Mysql, &format!("{}_idx", "a".repeat(70)))
    );
    assert_eq!(
        object_name(BackendKind::Postgres, &long),
        format!("\"{long}\"")
    );
    assert_eq!(object_name(BackendKind::Mysql, "t_c_idx"), "`t_c_idx`");
}

#[test]
fn auto_pk_ddl_per_backend() {
    let col = ColumnDef {
        name: "id".into(),
        col_type: ColumnType::AutoBigInt,
        nullable: false,
        primary_key: true,
        unique: true,
        default: None,
        references: None,
        index: false,
    };
    assert_eq!(
        column_definition("t", &col, BackendKind::Postgres),
        "\"id\" BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY"
    );
    assert_eq!(
        column_definition("t", &col, BackendKind::Mysql),
        "`id` BIGINT AUTO_INCREMENT PRIMARY KEY"
    );
    assert_eq!(
        column_definition("t", &col, BackendKind::Sqlite),
        "\"id\" INTEGER PRIMARY KEY"
    );
}

// ── SQLite rebuild ─────────────────────────────────────────────────────────
#[test]
fn sqlite_rebuild_table_emits_rebuild_statements() {
    let schema = TableSchema {
        table_name: "posts".to_string(),
        columns: vec![
            uuid_col("id", true),
            fk_col("author", "app_users", ReferentialAction::Cascade),
        ],
        ..Default::default()
    };
    let diff = SchemaDiff::RebuildTable {
        schema,
        copy: vec!["id".to_string(), "author".to_string()],
    };
    let sql = generate_sql_from_diff(&[diff], BackendKind::Sqlite);

    let create = sql
        .find("CREATE TABLE \"_rango_new_posts\" (")
        .expect("got: {sql}");
    let insert = sql
        .find("INSERT INTO \"_rango_new_posts\" (\"id\", \"author\") SELECT \"id\", \"author\" FROM \"posts\";")
        .expect("got: {sql}");
    let drop = sql.find("DROP TABLE \"posts\";").expect("got: {sql}");
    let rename = sql
        .find("ALTER TABLE \"_rango_new_posts\" RENAME TO \"posts\";")
        .expect("got: {sql}");
    let index = sql
        .find("CREATE INDEX IF NOT EXISTS \"posts_author_idx\" ON \"posts\" (\"author\");")
        .expect("got: {sql}");

    assert!(
        create < insert && insert < drop && drop < rename && rename < index,
        "got: {sql}"
    );
}

// ── foreign keys and pivots follow the target's primary key type ─────────
fn int_key(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef {
        name: name.to_string(),
        col_type: ty,
        nullable: false,
        primary_key: true,
        unique: true,
        default: None,
        references: None,
        index: false,
    }
}

#[test]
fn test_fk_to_bigint_pk_is_bigint_with_references_on_each_dialect() {
    use rango_orm_core::BackendKind::{Mysql, Postgres, Sqlite};
    let authors = TableSchema {
        table_name: "authors".to_string(),
        columns: vec![int_key("id", ColumnType::AutoBigInt)],
        ..Default::default()
    };
    let mut fk = fk_col("author_id", "authors", ReferentialAction::Cascade);
    fk.col_type = ColumnType::BigInt;
    let posts = TableSchema {
        table_name: "posts".to_string(),
        columns: vec![uuid_col("id", true), fk],
        ..Default::default()
    };
    for (backend, ty) in [(Postgres, "BIGINT"), (Mysql, "BIGINT"), (Sqlite, "INTEGER")] {
        let sql = generate_sql_from_diff(
            &[
                SchemaDiff::CreateTable(authors.clone()),
                SchemaDiff::CreateTable(posts.clone()),
            ],
            backend,
        );
        let q = |n: &str| backend.quote_ident(n);
        let col = format!("{} {ty} NOT NULL", q("author_id"));
        assert!(sql.contains(&col), "{backend:?}: missing `{col}` in {sql}");
        let refs = format!("REFERENCES {} ({})", q("authors"), q("id"));
        assert!(
            sql.contains(&refs),
            "{backend:?}: missing `{refs}` in {sql}"
        );
    }
}

#[test]
fn test_pivot_with_int_and_bigint_keys_has_composite_pk_on_each_dialect() {
    use rango_orm_core::BackendKind::{Mysql, Postgres, Sqlite};
    let schemas = [
        TableSchema {
            table_name: "posts".to_string(),
            columns: vec![int_key("id", ColumnType::AutoInt)],
            ..Default::default()
        },
        TableSchema {
            table_name: "tags".to_string(),
            columns: vec![int_key("id", ColumnType::AutoBigInt)],
            ..Default::default()
        },
    ];
    let rel = M2MRelation {
        from_table: "posts".to_string(),
        to_table: "tags".to_string(),
    };
    let pivot = generate_pivot_table(&rel, &schemas);
    assert_eq!(pivot.columns[0].col_type, ColumnType::Int);
    assert_eq!(pivot.columns[1].col_type, ColumnType::BigInt);
    for (backend, int, big) in [
        (Postgres, "INTEGER", "BIGINT"),
        (Mysql, "INT", "BIGINT"),
        (Sqlite, "INTEGER", "INTEGER"),
    ] {
        let sql = generate_sql_from_diff(&[SchemaDiff::CreateTable(pivot.clone())], backend);
        let q = |n: &str| backend.quote_ident(n);
        assert!(
            sql.contains(&format!("{} {int} NOT NULL", q("posts_id"))),
            "{backend:?}: {sql}"
        );
        assert!(
            sql.contains(&format!("{} {big} NOT NULL", q("tags_id"))),
            "{backend:?}: {sql}"
        );
        assert!(
            sql.contains(&format!(
                "PRIMARY KEY ({}, {})",
                q("posts_id"),
                q("tags_id")
            )),
            "{backend:?}: {sql}"
        );
        assert!(
            sql.contains(&format!("REFERENCES {} ({})", q("tags"), q("id"))),
            "{backend:?}: {sql}"
        );
    }
}
