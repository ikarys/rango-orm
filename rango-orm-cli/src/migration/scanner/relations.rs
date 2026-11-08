//! Foreign key resolution across scanned models, and M2M pivot tables.

use super::*;

pub(super) fn resolve_model_tables(
    schemas: &mut [TableSchema],
    m2m_relations: &mut [M2MRelation],
    model_tables: &HashMap<String, String>,
) {
    // A foreign key points at the target's primary key column, whatever its name, and
    // has that column's type (an auto-increment key is a plain INT / BIGINT in a FK).
    let pk_columns: HashMap<String, (String, ColumnType)> = schemas
        .iter()
        .filter_map(|s| {
            let pk = s.columns.iter().find(|c| c.primary_key)?;
            Some((
                s.table_name.clone(),
                (pk.name.clone(), fk_type(&pk.col_type)?),
            ))
        })
        .collect();
    for schema in schemas {
        for col in &mut schema.columns {
            if let Some(fk) = &mut col.references {
                fk.table = model_tables
                    .get(&fk.table)
                    .cloned()
                    .unwrap_or_else(|| fk.table.clone());
                if let Some((pk, ty)) = pk_columns.get(&fk.table) {
                    fk.column = pk.clone();
                    col.col_type = ty.clone();
                }
            }
        }
    }

    for rel in m2m_relations {
        rel.to_table = model_tables
            .get(&rel.to_table)
            .cloned()
            .unwrap_or_else(|| rel.to_table.clone());
    }
}

/// Column type of a foreign key (or pivot column) pointing at a primary key of type `pk`.
/// `None` for key types a relation cannot hold (the Rust side falls back to UUID).
pub(crate) fn fk_type(pk: &ColumnType) -> Option<ColumnType> {
    match pk {
        ColumnType::Uuid => Some(ColumnType::Uuid),
        ColumnType::Int | ColumnType::AutoInt => Some(ColumnType::Int),
        ColumnType::BigInt | ColumnType::AutoBigInt => Some(ColumnType::BigInt),
        _ => None,
    }
}

pub(super) fn to_snake_case(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(ch.to_lowercase().next().unwrap());
    }
    out
}

/// Generate a pivot table schema for a M2M relation. Each pivot column has the type of
/// the primary key it references (looked up in `schemas`; UUID `id` when unknown).
pub(crate) fn generate_pivot_table(rel: &M2MRelation, schemas: &[TableSchema]) -> TableSchema {
    let from_table = rel.from_table.clone();
    let to_table = rel.to_table.clone();
    // Same naming as the runtime, so the two cannot drift.
    let (pivot_name, from_col, to_col) = rango_orm_core::related::m2m_pivot(&from_table, &to_table);

    let key = |table: &str| -> (String, ColumnType) {
        schemas
            .iter()
            .find(|s| s.table_name == table)
            .and_then(|s| s.columns.iter().find(|c| c.primary_key))
            .and_then(|pk| Some((pk.name.clone(), fk_type(&pk.col_type)?)))
            .unwrap_or_else(|| ("id".to_string(), ColumnType::Uuid))
    };
    let column = |name: String, table: String, index: bool| {
        let (pk_name, col_type) = key(&table);
        rango_orm_core::ColumnDef {
            name,
            col_type,
            nullable: false,
            primary_key: false,
            unique: false,
            default: None,
            references: Some(rango_orm_core::FkReference {
                table,
                column: pk_name,
                on_delete: rango_orm_core::ReferentialAction::Cascade,
                on_update: rango_orm_core::ReferentialAction::NoAction,
            }),
            index,
        }
    };

    TableSchema {
        table_name: pivot_name,
        columns: vec![
            column(from_col, from_table, false),
            column(to_col, to_table, true),
        ],
        ..Default::default()
    }
}
