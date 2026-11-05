use crate::migration::snapshot::*;
use rango_core::{
    ColumnDef, FkReference, TableSchema,
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
