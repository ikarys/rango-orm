use anyhow::{Context, Result, bail};
use rango_orm_core::{
    CheckConstraint, ColumnDef, ColumnType, Constraint, DefaultValue, FkReference,
    ReferentialAction, TableSchema, UniqueConstraint,
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

impl Snapshot {
    /// Inverse of `from_schemas`, for a snapshot that comes from an untrusted file.
    /// Raw SQL fragments (defaults, CHECK conditions) must be a single statement.
    pub fn to_schemas(&self) -> Result<Vec<TableSchema>> {
        let mut names: Vec<&String> = self.tables.keys().collect();
        names.sort();
        names.into_iter().map(|n| self.table_schema(n)).collect()
    }

    fn table_schema(&self, name: &str) -> Result<TableSchema> {
        let table = &self.tables[name];
        let mut columns = Vec::new();
        for c in &table.columns {
            if let Some(d) = &c.default {
                single_statement(d)?;
            }
            columns.push(ColumnDef {
                name: c.name.clone(),
                col_type: parse_col_type(&c.col_type)?,
                nullable: c.nullable,
                primary_key: c.primary_key,
                unique: c.unique,
                default: c.default.as_deref().map(parse_default),
                index: c.index,
                references: c
                    .references
                    .as_ref()
                    .map(|r| {
                        Ok::<_, anyhow::Error>(FkReference {
                            table: r.table.clone(),
                            column: r.column.clone(),
                            on_delete: parse_referential_action(&r.on_delete)?,
                            on_update: ReferentialAction::NoAction,
                        })
                    })
                    .transpose()?,
            });
        }
        let constraints = table
            .constraints
            .iter()
            .map(parse_constraint)
            .collect::<Result<_>>()?;
        Ok(TableSchema {
            table_name: name.to_string(),
            columns,
            constraints,
            ..TableSchema::default()
        })
    }
}

fn single_statement(sql: &str) -> Result<()> {
    if sql.contains(';') || sql.contains("--") || sql.contains("/*") {
        bail!("Refusing SQL fragment with ';' or a comment: {}", sql);
    }
    Ok(())
}

fn parse_col_type(s: &str) -> Result<ColumnType> {
    let args = |prefix: &str| s.strip_prefix(prefix)?.strip_suffix(')');
    Ok(match s {
        "bool" => ColumnType::Bool,
        "smallint" => ColumnType::SmallInt,
        "int" => ColumnType::Int,
        "bigint" => ColumnType::BigInt,
        "autoint" => ColumnType::AutoInt,
        "autobigint" => ColumnType::AutoBigInt,
        "float" => ColumnType::Float,
        "double" => ColumnType::Double,
        "text" => ColumnType::Text,
        "bytea" => ColumnType::Bytea,
        "uuid" => ColumnType::Uuid,
        "date" => ColumnType::Date,
        "time" => ColumnType::Time,
        "datetime" => ColumnType::DateTime,
        "json" => ColumnType::Json,
        "jsonb" => ColumnType::Jsonb,
        _ => {
            if let Some(n) = args("varchar(").and_then(|n| n.parse().ok()) {
                ColumnType::Varchar(n)
            } else if let Some((p, sc)) = args("decimal(").and_then(|a| a.split_once(',')) {
                match (p.parse(), sc.parse()) {
                    (Ok(precision), Ok(scale)) => ColumnType::Decimal { precision, scale },
                    _ => bail!("Unknown column type '{}'", s),
                }
            } else {
                bail!("Unknown column type '{}'", s)
            }
        }
    })
}

fn parse_default(s: &str) -> DefaultValue {
    match s {
        "now()" => DefaultValue::CurrentTimestamp,
        "gen_random_uuid()" => DefaultValue::GeneratedUuid,
        _ => DefaultValue::Literal(s.to_string()),
    }
}

fn parse_referential_action(s: &str) -> Result<ReferentialAction> {
    Ok(match s {
        "cascade" => ReferentialAction::Cascade,
        "set_null" => ReferentialAction::SetNull,
        "set_default" => ReferentialAction::SetDefault,
        "restrict" => ReferentialAction::Restrict,
        "no_action" => ReferentialAction::NoAction,
        _ => bail!("Unknown referential action '{}'", s),
    })
}

/// Inverse of `constraint_definition`.
fn parse_constraint(c: &SnapshotConstraint) -> Result<Constraint> {
    let d = c.definition.as_str();
    if let Some(sql) = d.strip_prefix("CHECK (").and_then(|r| r.strip_suffix(')')) {
        single_statement(sql)?;
        return Ok(CheckConstraint::new(sql).name(&c.name).into());
    }
    let (cols, cond) = match d.strip_prefix("UNIQUE (") {
        Some(rest) => match rest.split_once(") WHERE ") {
            Some((cols, cond)) => (cols, Some(cond)),
            None => (rest.strip_suffix(')').unwrap_or(rest), None),
        },
        None => bail!("Unknown constraint definition '{}'", d),
    };
    let fields: Vec<String> = cols
        .split(", ")
        .map(|f| {
            f.strip_prefix('"')
                .and_then(|f| f.strip_suffix('"'))
                .map(|f| f.replace("\"\"", "\""))
                .with_context(|| format!("Bad column list in constraint '{}'", d))
        })
        .collect::<Result<_>>()?;
    let mut unique = UniqueConstraint {
        fields,
        condition: None,
        name: c.name.clone(),
    };
    if let Some(cond) = cond {
        single_statement(cond)?;
        unique.condition = Some(cond.to_string());
    }
    Ok(unique.into())
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

    #[test]
    fn to_schemas_round_trips_and_rejects_injected_sql() {
        let json = r#"{"tables": {"t": {"columns": [
            {"name": "id", "col_type": "varchar(20)", "nullable": false, "primary_key": true,
             "unique": true, "default": null}],
            "constraints": [{"name": "c", "definition": "CHECK (id <> 'x')"},
                            {"name": "u", "definition": "UNIQUE (\"a\", \"b\") WHERE a > 1"}]}}}"#;
        let snap: Snapshot = serde_json::from_str(json).unwrap();
        let schemas = snap.to_schemas().unwrap();
        let again = Snapshot::from_schemas(&schemas);
        assert_eq!(again.tables["t"].constraints, snap.tables["t"].constraints);
        assert_eq!(again.tables["t"].columns, snap.tables["t"].columns);

        let evil = json.replace("id <> 'x'", "1); DROP TABLE t; --");
        let snap: Snapshot = serde_json::from_str(&evil).unwrap();
        assert!(snap.to_schemas().is_err());
    }
}
