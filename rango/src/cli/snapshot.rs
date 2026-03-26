use anyhow::{Context, Result};
use rango_core::{ColumnDef, ColumnType, DefaultValue, TableSchema};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const SNAPSHOT_FILE: &str = "snapshot.json";

/// Persisted state of all known tables.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Snapshot {
    pub tables: HashMap<String, SnapshotTable>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotTable {
    pub columns: Vec<SnapshotColumn>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotColumn {
    pub name: String,
    pub col_type: String, // serialized as string for simplicity
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<String>,
}

impl Snapshot {
    pub fn load(migrations_dir: &str) -> Result<Self> {
        let path = Path::new(migrations_dir).join(SNAPSHOT_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str(&content).context("Failed to parse snapshot.json")
    }

    pub fn save(&self, migrations_dir: &str) -> Result<()> {
        let path = Path::new(migrations_dir).join(SNAPSHOT_FILE);
        let content = serde_json::to_string_pretty(self).context("Failed to serialize snapshot")?;
        fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))
    }

    /// Build a snapshot from current model schemas.
    pub fn from_schemas(schemas: &[TableSchema]) -> Self {
        let mut tables = HashMap::new();
        for schema in schemas {
            let columns = schema
                .columns
                .iter()
                .map(|c| SnapshotColumn {
                    name: c.name.clone(),
                    col_type: serialize_col_type(&c.col_type),
                    nullable: c.nullable,
                    primary_key: c.primary_key,
                    unique: c.unique,
                    default: c.default.as_ref().map(serialize_default),
                })
                .collect();
            tables.insert(schema.table_name.clone(), SnapshotTable { columns });
        }
        Self { tables }
    }
}

fn serialize_col_type(t: &ColumnType) -> String {
    match t {
        ColumnType::Bool => "bool".into(),
        ColumnType::SmallInt => "smallint".into(),
        ColumnType::Int => "int".into(),
        ColumnType::BigInt => "bigint".into(),
        ColumnType::Float => "float".into(),
        ColumnType::Double => "double".into(),
        ColumnType::Text => "text".into(),
        ColumnType::Bytea => "bytea".into(),
        ColumnType::Uuid => "uuid".into(),
        ColumnType::Date => "date".into(),
        ColumnType::Time => "time".into(),
        ColumnType::DateTime => "datetime".into(),
        ColumnType::Json => "json".into(),
        ColumnType::Jsonb => "jsonb".into(),
        ColumnType::Varchar(n) => format!("varchar({})", n),
        ColumnType::Decimal { precision, scale } => format!("decimal({},{})", precision, scale),
    }
}

fn serialize_default(d: &DefaultValue) -> String {
    match d {
        DefaultValue::CurrentTimestamp => "now()".into(),
        DefaultValue::GeneratedUuid => "gen_random_uuid()".into(),
        DefaultValue::Literal(s) => s.clone(),
    }
}

/// A single change between old and new schema state.
#[derive(Debug)]
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
        column: String,
        new_type: ColumnType,
    },
    AlterColumnNullable {
        table: String,
        column: String,
        nullable: bool,
    },
    AlterColumnUnique {
        table: String,
        column: String,
        unique: bool,
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
                        let new_type_str = serialize_col_type(&col.col_type);
                        if new_type_str != old_col.col_type {
                            diffs.push(SchemaDiff::AlterColumnType {
                                table: schema.table_name.clone(),
                                column: col.name.clone(),
                                new_type: col.col_type.clone(),
                            });
                        }
                        if col.nullable != old_col.nullable {
                            diffs.push(SchemaDiff::AlterColumnNullable {
                                table: schema.table_name.clone(),
                                column: col.name.clone(),
                                nullable: col.nullable,
                            });
                        }
                        if col.unique != old_col.unique && !col.primary_key {
                            diffs.push(SchemaDiff::AlterColumnUnique {
                                table: schema.table_name.clone(),
                                column: col.name.clone(),
                                unique: col.unique,
                            });
                        }
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
