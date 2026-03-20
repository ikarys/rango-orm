/// Represents the schema of a single table, as declared by a Model.
#[derive(Debug, Clone)]
pub struct TableSchema {
    pub table_name: String,
    pub columns: Vec<ColumnDef>,
}

/// Definition of a single column.
#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: String,
    pub col_type: ColumnType,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<DefaultValue>,
    pub references: Option<ForeignKey>,
}

/// Common column types, backend-agnostic.
/// Backends map these to their native SQL types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnType {
    Bool,
    SmallInt,
    Int,
    BigInt,
    Float,
    Double,
    Decimal { precision: u8, scale: u8 },
    Text,
    Varchar(u32),
    Bytea,
    Uuid,
    Date,
    Time,
    DateTime,
    Json,
    Jsonb, // Postgres-specific — backends that don't support it fall back to Json
}

#[derive(Debug, Clone)]
pub enum DefaultValue {
    Literal(String),
    CurrentTimestamp,
    GeneratedUuid,
}

/// Foreign key reference.
#[derive(Debug, Clone)]
pub struct ForeignKey {
    pub table: String,
    pub column: String,
    pub on_delete: ReferentialAction,
    pub on_update: ReferentialAction,
}

/// Standard SQL referential actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferentialAction {
    Cascade,
    SetNull,
    SetDefault,
    Restrict,
    NoAction,
}
