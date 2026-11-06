use crate::migration::diff::{SchemaDiff, column_diffs};
use crate::migration::snapshot::*;
use rango_orm_core::ColumnDef;
use std::collections::HashSet;

/// Detect column renames from paired DropColumn/AddColumn diffs.
pub fn detect_renames(
    diffs: Vec<SchemaDiff>,
    old_snapshot: &Snapshot,
    mut confirm: impl FnMut(&str, &str, &str) -> bool,
) -> Vec<SchemaDiff> {
    let mut tables: Vec<String> = Vec::new();
    for d in &diffs {
        let table = match d {
            SchemaDiff::AddColumn { table, .. } => table,
            SchemaDiff::DropColumn { table, .. } => table,
            _ => continue,
        };
        if !tables.contains(table) {
            tables.push(table.clone());
        }
    }

    let mut rename_diffs: Vec<SchemaDiff> = Vec::new();
    let mut used_adds: HashSet<(String, String)> = HashSet::new();
    let mut used_drops: HashSet<(String, String)> = HashSet::new();

    for table in &tables {
        let drops: Vec<String> = diffs
            .iter()
            .filter_map(|d| match d {
                SchemaDiff::DropColumn { table: t, column } if t == table => Some(column.clone()),
                _ => None,
            })
            .collect();
        let adds: Vec<ColumnDef> = diffs
            .iter()
            .filter_map(|d| match d {
                SchemaDiff::AddColumn { table: t, column } if t == table => Some(column.clone()),
                _ => None,
            })
            .collect();

        let mut candidates: Vec<(usize, usize, bool)> = Vec::new();
        for (di, drop) in drops.iter().enumerate() {
            for (ai, add) in adds.iter().enumerate() {
                let old_col = old_snapshot
                    .tables
                    .get(table)
                    .and_then(|t| t.columns.iter().find(|c| &c.name == drop));
                let same_type = old_col
                    .map(|c| c.col_type == serialize_col_type(&add.col_type))
                    .unwrap_or(false);
                candidates.push((di, ai, same_type));
            }
        }

        candidates.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));

        let mut local_used_drops: HashSet<usize> = HashSet::new();
        let mut local_used_adds: HashSet<usize> = HashSet::new();

        for (di, ai, _) in candidates {
            if local_used_drops.contains(&di) || local_used_adds.contains(&ai) {
                continue;
            }

            let drop = &drops[di];
            let add = &adds[ai];

            if !confirm(table, drop, &add.name) {
                continue;
            }

            local_used_drops.insert(di);
            local_used_adds.insert(ai);
            used_drops.insert((table.clone(), drop.clone()));
            used_adds.insert((table.clone(), add.name.clone()));

            rename_diffs.push(SchemaDiff::RenameColumn {
                table: table.clone(),
                from: drop.clone(),
                to: add.name.clone(),
            });

            if let Some(old_col) = old_snapshot
                .tables
                .get(table)
                .and_then(|t| t.columns.iter().find(|c| &c.name == drop))
            {
                rename_diffs.extend(column_diffs(table, old_col, add));
            }
        }
    }

    let mut result = rename_diffs;
    for d in &diffs {
        match d {
            SchemaDiff::AddColumn { table, column } => {
                if !used_adds.contains(&(table.clone(), column.name.clone())) {
                    result.push(d.clone());
                }
            }
            SchemaDiff::DropColumn { table, column } => {
                if !used_drops.contains(&(table.clone(), column.clone())) {
                    result.push(d.clone());
                }
            }
            _ => result.push(d.clone()),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::diff::SchemaDiff;
    use rango_orm_core::{ColumnDef, ColumnType};
    use std::collections::HashMap;

    fn old_snapshot_with_amount() -> Snapshot {
        Snapshot {
            tables: HashMap::from([(
                "t".to_string(),
                SnapshotTable {
                    columns: vec![SnapshotColumn {
                        name: "amount".to_string(),
                        col_type: "text".to_string(),
                        nullable: false,
                        primary_key: false,
                        unique: false,
                        default: None,
                        index: false,
                        references: None,
                    }],
                    constraints: Vec::new(),
                },
            )]),
        }
    }

    #[test]
    fn test_detect_renames_confirmed() {
        let old = old_snapshot_with_amount();
        let diffs = vec![
            SchemaDiff::DropColumn {
                table: "t".to_string(),
                column: "amount".to_string(),
            },
            SchemaDiff::AddColumn {
                table: "t".to_string(),
                column: ColumnDef {
                    name: "price".to_string(),
                    col_type: ColumnType::Int,
                    nullable: false,
                    primary_key: false,
                    unique: false,
                    default: None,
                    index: false,
                    references: None,
                },
            },
        ];

        let renamed = detect_renames(diffs, &old, |_, _, _| true);

        assert_eq!(renamed.len(), 2);
        match &renamed[0] {
            SchemaDiff::RenameColumn { table, from, to } => {
                assert_eq!(table, "t");
                assert_eq!(from, "amount");
                assert_eq!(to, "price");
            }
            other => panic!("expected RenameColumn, got {:?}", other),
        }
        match &renamed[1] {
            SchemaDiff::AlterColumnType { table, column } => {
                assert_eq!(table, "t");
                assert_eq!(column.name, "price");
                assert_eq!(column.col_type, ColumnType::Int);
            }
            other => panic!("expected AlterColumnType, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_renames_not_confirmed() {
        let old = old_snapshot_with_amount();
        let diffs = vec![
            SchemaDiff::DropColumn {
                table: "t".to_string(),
                column: "amount".to_string(),
            },
            SchemaDiff::AddColumn {
                table: "t".to_string(),
                column: ColumnDef {
                    name: "price".to_string(),
                    col_type: ColumnType::Int,
                    nullable: false,
                    primary_key: false,
                    unique: false,
                    default: None,
                    index: false,
                    references: None,
                },
            },
        ];

        let unchanged = detect_renames(diffs, &old, |_, _, _| false);

        assert_eq!(unchanged.len(), 2);
        assert!(matches!(unchanged[0], SchemaDiff::DropColumn { .. }));
        assert!(matches!(unchanged[1], SchemaDiff::AddColumn { .. }));
    }
}
