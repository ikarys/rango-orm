use crate::migration::snapshot::*;
use rango_orm_core::{
    ColumnDef, ColumnType, FkReference, TableSchema,
    constraints::{constraint_definition, constraint_name},
};
use std::collections::{HashMap, HashSet};

/// A single change between old and new schema state.
#[derive(Debug, Clone)]
pub enum SchemaDiff {
    CreateTable(TableSchema),
    DropTable(String),
    AddColumn {
        table: String,
        column: ColumnDef,
    },
    DropColumn {
        table: String,
        column: String,
    },
    AlterColumnType {
        table: String,
        column: ColumnDef,
    },
    AlterColumnNullable {
        table: String,
        column: ColumnDef,
    },
    AlterColumnUnique {
        table: String,
        column: String,
        unique: bool,
    },
    AlterColumnIndex {
        table: String,
        column: String,
        /// Needed by MySQL, which cannot index TEXT/BLOB without a prefix length.
        col_type: ColumnType,
        index: bool,
    },
    AlterColumnReference {
        table: String,
        column: String,
        references: Option<FkReference>,
        had_reference: bool,
    },
    AddConstraint {
        table: String,
        constraint: SnapshotConstraint,
    },
    DropConstraint {
        table: String,
        name: String,
        partial: bool,
    },
    RenameColumn {
        table: String,
        from: String,
        to: String,
    },
    /// SQLite only: recreate `schema.table_name` (create new → copy `copy` columns → drop → rename).
    RebuildTable {
        schema: TableSchema,
        copy: Vec<String>,
    },
}

/// Compute the diff between old snapshot and current schemas.
pub fn diff(old: &Snapshot, new_schemas: &[TableSchema]) -> Vec<SchemaDiff> {
    let mut diffs = Vec::new();

    // New tables and column changes
    for schema in new_schemas {
        match old.tables.get(&schema.table_name) {
            None => {
                // Brand new table
                diffs.push(SchemaDiff::CreateTable(schema.clone()));
            }
            Some(old_table) => {
                let old_cols: HashMap<&str, &SnapshotColumn> = old_table
                    .columns
                    .iter()
                    .map(|c| (c.name.as_str(), c))
                    .collect();
                let new_cols: HashMap<&str, &ColumnDef> = schema
                    .columns
                    .iter()
                    .map(|c| (c.name.as_str(), c))
                    .collect();

                // Added columns
                for col in &schema.columns {
                    if !old_cols.contains_key(col.name.as_str()) {
                        diffs.push(SchemaDiff::AddColumn {
                            table: schema.table_name.clone(),
                            column: col.clone(),
                        });
                    }
                }

                // Dropped columns
                for old_col in &old_table.columns {
                    if !new_cols.contains_key(old_col.name.as_str()) {
                        diffs.push(SchemaDiff::DropColumn {
                            table: schema.table_name.clone(),
                            column: old_col.name.clone(),
                        });
                    }
                }

                // Changed columns
                for col in &schema.columns {
                    if let Some(old_col) = old_cols.get(col.name.as_str()) {
                        diffs.extend(column_diffs(&schema.table_name, old_col, col));
                    }
                }

                // Added constraints
                let old_constraint_names: HashSet<&str> = old_table
                    .constraints
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect();
                for (i, constraint) in schema.constraints.iter().enumerate() {
                    let name = constraint_name(&schema.table_name, constraint, i);
                    if !old_constraint_names.contains(name.as_str()) {
                        diffs.push(SchemaDiff::AddConstraint {
                            table: schema.table_name.clone(),
                            constraint: SnapshotConstraint {
                                name,
                                definition: constraint_definition(constraint),
                            },
                        });
                    }
                }

                // Dropped constraints
                let new_constraint_names: HashSet<String> = schema
                    .constraints
                    .iter()
                    .enumerate()
                    .map(|(i, c)| constraint_name(&schema.table_name, c, i))
                    .collect();
                for old_constraint in &old_table.constraints {
                    if !new_constraint_names.contains(&old_constraint.name) {
                        diffs.push(SchemaDiff::DropConstraint {
                            table: schema.table_name.clone(),
                            name: old_constraint.name.clone(),
                            partial: old_constraint.definition.contains(" WHERE "),
                        });
                    }
                }
            }
        }
    }

    // Dropped tables
    let new_table_names: std::collections::HashSet<&str> =
        new_schemas.iter().map(|s| s.table_name.as_str()).collect();
    for table_name in old.tables.keys() {
        if !new_table_names.contains(table_name.as_str()) {
            diffs.push(SchemaDiff::DropTable(table_name.clone()));
        }
    }

    diffs
}

pub fn sqlite_rebuilds(diffs: Vec<SchemaDiff>, schemas: &[TableSchema]) -> Vec<SchemaDiff> {
    let mut rebuild_tables: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for d in &diffs {
        if let Some(table) = sqlite_rebuild_trigger(d)
            && seen.insert(table)
        {
            rebuild_tables.push(table.to_string());
        }
    }

    let rebuild_tables: Vec<String> = rebuild_tables
        .into_iter()
        .filter(|t| schemas.iter().any(|s| &s.table_name == t))
        .collect();
    let rebuild_set: HashSet<&str> = rebuild_tables.iter().map(|s| s.as_str()).collect();

    let mut result = Vec::with_capacity(diffs.len());
    for d in &diffs {
        if let Some(table) = diff_table(d)
            && rebuild_set.contains(table)
            && !matches!(d, SchemaDiff::RenameColumn { .. })
        {
            continue;
        }
        result.push(d.clone());
    }

    for table in &rebuild_tables {
        if let Some(schema) = schemas.iter().find(|s| &s.table_name == table) {
            let added: HashSet<&str> = diffs
                .iter()
                .filter_map(|d| match d {
                    SchemaDiff::AddColumn { table: t, column } if t == table => {
                        Some(column.name.as_str())
                    }
                    _ => None,
                })
                .collect();
            let copy = schema
                .columns
                .iter()
                .map(|c| c.name.clone())
                .filter(|name| !added.contains(name.as_str()))
                .collect();
            result.push(SchemaDiff::RebuildTable {
                schema: schema.clone(),
                copy,
            });
        }
    }

    result
}

fn sqlite_rebuild_trigger(d: &SchemaDiff) -> Option<&str> {
    match d {
        SchemaDiff::AlterColumnType { table, .. }
        | SchemaDiff::AlterColumnNullable { table, .. }
        | SchemaDiff::AlterColumnReference { table, .. }
        | SchemaDiff::AlterColumnUnique { table, .. } => Some(table),
        SchemaDiff::AddConstraint { table, constraint }
            if !constraint.definition.contains(" WHERE ") =>
        {
            Some(table)
        }
        SchemaDiff::DropConstraint { table, partial, .. } if !*partial => Some(table),
        _ => None,
    }
}

fn diff_table(d: &SchemaDiff) -> Option<&str> {
    match d {
        SchemaDiff::CreateTable(schema) => Some(&schema.table_name),
        SchemaDiff::DropTable(table) => Some(table),
        SchemaDiff::AddColumn { table, .. }
        | SchemaDiff::DropColumn { table, .. }
        | SchemaDiff::AlterColumnType { table, .. }
        | SchemaDiff::AlterColumnNullable { table, .. }
        | SchemaDiff::AlterColumnUnique { table, .. }
        | SchemaDiff::AlterColumnIndex { table, .. }
        | SchemaDiff::AlterColumnReference { table, .. }
        | SchemaDiff::AddConstraint { table, .. }
        | SchemaDiff::DropConstraint { table, .. }
        | SchemaDiff::RenameColumn { table, .. } => Some(table),
        SchemaDiff::RebuildTable { schema, .. } => Some(&schema.table_name),
    }
}

pub(crate) fn column_diffs(table: &str, old: &SnapshotColumn, new: &ColumnDef) -> Vec<SchemaDiff> {
    let mut diffs = Vec::new();

    let new_type_str = serialize_col_type(&new.col_type);
    if new_type_str != old.col_type {
        diffs.push(SchemaDiff::AlterColumnType {
            table: table.to_string(),
            column: new.clone(),
        });
    }

    if new.nullable != old.nullable {
        diffs.push(SchemaDiff::AlterColumnNullable {
            table: table.to_string(),
            column: new.clone(),
        });
    }

    if new.unique != old.unique && !new.primary_key {
        diffs.push(SchemaDiff::AlterColumnUnique {
            table: table.to_string(),
            column: new.name.clone(),
            unique: new.unique,
        });
    }

    if new.index != old.index {
        diffs.push(SchemaDiff::AlterColumnIndex {
            table: table.to_string(),
            column: new.name.clone(),
            col_type: new.col_type.clone(),
            index: new.index,
        });
    }

    if references_changed(old.references.as_ref(), new.references.as_ref()) {
        diffs.push(SchemaDiff::AlterColumnReference {
            table: table.to_string(),
            column: new.name.clone(),
            references: new.references.clone(),
            had_reference: old.references.is_some(),
        });
    }

    diffs
}

fn references_changed(old: Option<&SnapshotFk>, new: Option<&FkReference>) -> bool {
    match (old, new) {
        (None, None) => false,
        (Some(o), Some(n)) => {
            o.table != n.table
                || o.column != n.column
                || o.on_delete != serialize_referential_action(&n.on_delete)
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rango_orm_core::{ColumnDef, ColumnType};

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

    fn simple_table(name: &str) -> TableSchema {
        TableSchema {
            table_name: name.to_string(),
            columns: vec![text_col("id"), text_col("name")],
            ..Default::default()
        }
    }

    #[test]
    fn sqlite_rebuilds_replaces_alter_with_rebuild() {
        let schema = TableSchema {
            table_name: "t".to_string(),
            columns: vec![text_col("id"), text_col("name"), text_col("new_col")],
            ..Default::default()
        };
        let diffs = vec![
            SchemaDiff::AlterColumnNullable {
                table: "t".to_string(),
                column: text_col("name"),
            },
            SchemaDiff::AddColumn {
                table: "t".to_string(),
                column: text_col("new_col"),
            },
            SchemaDiff::DropColumn {
                table: "t".to_string(),
                column: "old_col".to_string(),
            },
            SchemaDiff::RenameColumn {
                table: "t".to_string(),
                from: "old_name".to_string(),
                to: "name".to_string(),
            },
        ];
        let result = sqlite_rebuilds(diffs, &[schema]);
        assert_eq!(result.len(), 2);
        assert!(matches!(result[0], SchemaDiff::RenameColumn { .. }));
        match &result[1] {
            SchemaDiff::RebuildTable { schema, copy } => {
                assert_eq!(schema.table_name, "t");
                assert_eq!(copy, &vec!["id".to_string(), "name".to_string()]);
            }
            other => panic!("expected RebuildTable, got {other:?}"),
        }
    }

    #[test]
    fn sqlite_rebuilds_leaves_non_rebuild_diffs_untouched() {
        let schema = simple_table("t");
        let diffs = vec![
            SchemaDiff::AddColumn {
                table: "t".to_string(),
                column: text_col("new_col"),
            },
            SchemaDiff::AlterColumnIndex {
                table: "t".to_string(),
                column: "new_col".to_string(),
                col_type: ColumnType::Text,
                index: true,
            },
        ];
        let result = sqlite_rebuilds(diffs, &[schema]);
        assert_eq!(result.len(), 2);
        assert!(matches!(result[0], SchemaDiff::AddColumn { .. }));
        assert!(matches!(result[1], SchemaDiff::AlterColumnIndex { .. }));
    }

    #[test]
    fn sqlite_rebuilds_partial_drop_constraint_does_not_rebuild() {
        let schema = simple_table("t");
        let diffs = vec![SchemaDiff::DropConstraint {
            table: "t".to_string(),
            name: "t_x".to_string(),
            partial: true,
        }];
        let result = sqlite_rebuilds(diffs, &[schema]);
        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0],
            SchemaDiff::DropConstraint { partial: true, .. }
        ));
    }
}
