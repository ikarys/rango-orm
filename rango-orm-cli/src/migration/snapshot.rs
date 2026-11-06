use anyhow::{Context, Result};
use rango_orm_core::{
    ColumnType, DefaultValue, ReferentialAction, TableSchema,
    constraints::{constraint_definition, constraint_name},
};
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
    #[serde(default)]
    pub constraints: Vec<SnapshotConstraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotColumn {
    pub name: String,
    pub col_type: String, // serialized as string for simplicity
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<String>,
    #[serde(default)]
    pub index: bool,
    #[serde(default)]
    pub references: Option<SnapshotFk>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotFk {
    pub table: String,
    pub column: String,
    pub on_delete: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotConstraint {
    pub name: String,
    pub definition: String,
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
                    index: c.index,
                    references: c.references.as_ref().map(|r| SnapshotFk {
                        table: r.table.clone(),
                        column: r.column.clone(),
                        on_delete: serialize_referential_action(&r.on_delete),
                    }),
                })
                .collect();
            let constraints = schema
                .constraints
                .iter()
                .enumerate()
                .map(|(i, c)| SnapshotConstraint {
                    name: constraint_name(&schema.table_name, c, i),
                    definition: constraint_definition(c),
                })
                .collect();
            tables.insert(
                schema.table_name.clone(),
                SnapshotTable {
                    columns,
                    constraints,
                },
            );
        }
        Self { tables }
    }
}

pub(crate) fn serialize_col_type(t: &ColumnType) -> String {
    match t {
        ColumnType::Bool => "bool".into(),
        ColumnType::SmallInt => "smallint".into(),
        ColumnType::Int => "int".into(),
        ColumnType::BigInt => "bigint".into(),
        ColumnType::AutoInt => "autoint".into(),
        ColumnType::AutoBigInt => "autobigint".into(),
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

pub(crate) fn serialize_default(d: &DefaultValue) -> String {
    match d {
        DefaultValue::CurrentTimestamp => "now()".into(),
        DefaultValue::GeneratedUuid => "gen_random_uuid()".into(),
        DefaultValue::Literal(s) => s.clone(),
    }
}

pub(crate) fn serialize_referential_action(a: &ReferentialAction) -> String {
    match a {
        ReferentialAction::Cascade => "cascade".into(),
        ReferentialAction::SetNull => "set_null".into(),
        ReferentialAction::SetDefault => "set_default".into(),
        ReferentialAction::Restrict => "restrict".into(),
        ReferentialAction::NoAction => "no_action".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_old_snapshot_json_without_new_fields_deserializes() {
        let json = r#"{
            "tables": {
                "users": {
                    "columns": [
                        {
                            "name": "id",
                            "col_type": "uuid",
                            "nullable": false,
                            "primary_key": true,
                            "unique": true,
                            "default": null
                        }
                    ]
                }
            }
        }"#;

        let snapshot: Snapshot = serde_json::from_str(json).unwrap();
        let table = snapshot.tables.get("users").unwrap();
        assert!(table.constraints.is_empty());

        let col = &table.columns[0];
        assert!(!col.index);
        assert!(col.references.is_none());
    }
}
