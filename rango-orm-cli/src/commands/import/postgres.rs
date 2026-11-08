//! Postgres import: `jsonb_populate_record` inserts, deferred constraints, identity resync.

use super::binary::hex_encode;
use super::*;

/// Binary (`bytea`) columns of a Postgres table.
pub(crate) async fn binary_columns_postgres(
    conn: &mut sqlx::PgConnection,
    table: &str,
) -> Result<HashSet<String>> {
    columns_of_type_postgres(conn, table, "bytea").await
}

/// Columns of a Postgres table whose `information_schema` data type is `data_type`.
pub(crate) async fn columns_of_type_postgres(
    conn: &mut sqlx::PgConnection,
    table: &str,
    data_type: &str,
) -> Result<HashSet<String>> {
    let cols: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(
        "SELECT column_name::text FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = $1 AND data_type = $2",
    ))
    .bind(table)
    .bind(data_type)
    .fetch_all(conn)
    .await
    .context("Failed to read column types")?;
    Ok(cols.into_iter().collect())
}

pub(super) async fn import_postgres(
    database_url: &str,
    rows: &[serde_json::Map<String, serde_json::Value>],
    table: Option<&str>,
    on_conflict: OnConflict,
    setup_sql: Option<&str>,
) -> Result<usize> {
    let pool = sqlx::PgPool::connect(database_url)
        .await
        .context("Failed to connect to Postgres")?;

    // Postgres cannot disable FK checks without superuser, so parents are inserted
    // first. One transaction, so a failed import leaves nothing behind.
    let mut tx = pool.begin().await.context("Failed to start transaction")?;
    run_setup::<sqlx::Postgres>(&mut tx, setup_sql).await?;
    // Constraints created `DEFERRABLE` (the FKs that close a cycle) are checked at commit,
    // so a NOT NULL cycle loads whatever the row order. The others stay immediate.
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *tx)
        .await
        .context("Failed to defer constraints")?;
    let edges = fk_edges_postgres(&mut tx).await?;

    // Group rows by table
    let groups = group_by_table(rows, table);
    let order = order_by_dependencies(groups.keys().cloned().collect(), &edges);
    let mut total = 0;

    // Nullable FK columns that point at a table not inserted yet (cycles, self references)
    // are inserted as NULL and filled in once every row is there.
    let nullable_fks = nullable_fk_columns_postgres(&mut tx).await?;
    let position: HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, t)| (t.as_str(), i))
        .collect();
    let mut fixups: Vec<(
        String,
        Vec<String>,
        serde_json::Map<String, serde_json::Value>,
    )> = Vec::new();
    for (i, table) in order.iter().enumerate() {
        let deferred: Vec<String> = nullable_fks
            .iter()
            .filter(|(t, _, parent)| {
                t == table && position.get(parent.as_str()).is_some_and(|&p| p >= i)
            })
            .map(|(_, col, _)| col.clone())
            .collect();
        let mut rows_here: Vec<serde_json::Map<String, serde_json::Value>> =
            groups[table].iter().map(|r| (*r).clone()).collect();
        if !deferred.is_empty() && on_conflict == OnConflict::Fail {
            let pk = primary_key_postgres(&mut tx, table).await?;
            for row in &mut rows_here {
                let mut fix = serde_json::Map::new();
                for c in &deferred {
                    if row.get(c).is_some_and(|v| !v.is_null()) {
                        fix.insert(
                            c.clone(),
                            row.insert(c.clone(), serde_json::Value::Null).unwrap(),
                        );
                    }
                }
                if !fix.is_empty() && !pk.is_empty() {
                    for k in &pk {
                        fix.insert(k.clone(), row.get(k).cloned().unwrap_or_default());
                    }
                    fixups.push((table.clone(), pk.clone(), fix));
                }
            }
        }
        let refs: Vec<&serde_json::Map<String, serde_json::Value>> = rows_here.iter().collect();
        total += insert_rows_postgres(&mut tx, table, &refs, on_conflict).await?;
    }
    for (table, pk, fix) in &fixups {
        let rec = format!(
            "jsonb_populate_record(NULL::{}, $1::jsonb)",
            quote_ident(table)
        );
        let set = fix
            .keys()
            .filter(|k| !pk.contains(k))
            .map(|c| format!("{q} = (r).{q}", q = quote_ident(c)))
            .collect::<Vec<_>>()
            .join(", ");
        let cond = pk
            .iter()
            .map(|k| {
                format!(
                    "{t}.{q} = (r).{q}",
                    t = quote_ident(table),
                    q = quote_ident(k)
                )
            })
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!(
            "UPDATE {t} SET {set} FROM (SELECT {rec} AS r) x WHERE {cond}",
            t = quote_ident(table)
        );
        sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(serde_json::Value::Object(fix.clone()).to_string())
            .execute(&mut *tx)
            .await
            .with_context(|| format!("Failed to set deferred foreign keys of {}", table))?;
    }
    resync_identities_postgres(&mut tx, &order).await?;

    tx.commit().await.context("Failed to commit import")?;
    Ok(total)
}

/// Move each identity sequence of `tables` past the largest imported id.
pub(super) async fn resync_identities_postgres(
    conn: &mut sqlx::PgConnection,
    tables: &[String],
) -> Result<()> {
    let identities: Vec<(String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(
        "SELECT table_name::text, column_name::text FROM information_schema.columns \
         WHERE table_schema = current_schema() AND is_identity = 'YES'",
    ))
    .fetch_all(&mut *conn)
    .await
    .context("Failed to read identity columns")?;
    for (table, column) in identities.iter().filter(|(t, _)| tables.contains(t)) {
        let sql = format!(
            "SELECT setval(pg_get_serial_sequence($1, $2), COALESCE((SELECT MAX({}) FROM {}), 0) + 1, false)",
            quote_ident(column),
            quote_ident(table)
        );
        sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(quote_ident(table))
            .bind(column)
            .execute(&mut *conn)
            .await
            .with_context(|| format!("Failed to reset the identity sequence of {}", table))?;
    }
    Ok(())
}

pub(super) async fn insert_rows_postgres(
    conn: &mut sqlx::PgConnection,
    table: &str,
    rows: &[&serde_json::Map<String, serde_json::Value>],
    on_conflict: OnConflict,
) -> Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }

    let cols = insert_columns(rows[0]);

    let col_list = cols
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    let conflict = match on_conflict {
        OnConflict::Replace => {
            // Postgres needs a conflict target for DO UPDATE: the primary key.
            let pk: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(
                "SELECT a.attname::text FROM pg_index i \
                 JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) \
                 WHERE i.indrelid = $1::regclass AND i.indisprimary",
            ))
            .bind(quote_ident(table))
            .fetch_all(&mut *conn)
            .await
            .with_context(|| format!("Failed to read the primary key of {}", table))?;
            if pk.is_empty() {
                bail!("--replace needs a primary key on table {}", table);
            }
            let updates = cols
                .iter()
                .map(|c| format!("{} = EXCLUDED.{}", quote_ident(c), quote_ident(c)))
                .collect::<Vec<_>>()
                .join(", ");
            let target = pk
                .iter()
                .map(|c| quote_ident(c))
                .collect::<Vec<_>>()
                .join(", ");
            format!("ON CONFLICT ({}) DO UPDATE SET {}", target, updates)
        }
        OnConflict::Skip => "ON CONFLICT DO NOTHING".to_string(),
        OnConflict::Fail => String::new(),
    };
    // The row travels as one JSON parameter; Postgres casts each value to the column
    // type (uuid, timestamps, booleans, ...). Names are quoted, values are bound.
    let sql = format!(
        "INSERT INTO {t} ({cols}) SELECT {cols} FROM jsonb_populate_record(NULL::{t}, $1::jsonb) WHERE true {conflict}",
        t = quote_ident(table),
        cols = col_list,
    );

    let binary = binary_columns_postgres(&mut *conn, table).await?;
    let mut count = 0;
    for row in rows {
        let mut values = serde_json::Map::new();
        for c in &cols {
            let mut v = row.get(*c).cloned().unwrap_or(serde_json::Value::Null);
            if let (true, serde_json::Value::String(s)) = (binary.contains(*c), &v) {
                // jsonb_populate_record reads bytea in its `\x` hex text form.
                v = serde_json::Value::String(format!("\\x{}", hex_encode(&decode_binary(c, s)?)));
            }
            values.insert(c.to_string(), v);
        }
        sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(serde_json::Value::Object(values).to_string())
            .execute(&mut *conn)
            .await
            .with_context(|| format!("Failed to insert into {}", table))?;
        count += 1;
    }
    Ok(count)
}

/// Single-column nullable foreign keys of the current schema, as `(table, column, parent)`.
async fn nullable_fk_columns_postgres(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<(String, String, String)>> {
    let sql = "SELECT c.relname::text, a.attname::text, p.relname::text
FROM pg_constraint k
JOIN pg_class c ON c.oid = k.conrelid
JOIN pg_class p ON p.oid = k.confrelid
JOIN pg_namespace n ON n.oid = c.relnamespace
JOIN pg_attribute a ON a.attrelid = k.conrelid AND a.attnum = k.conkey[1]
WHERE k.contype = 'f' AND array_length(k.conkey, 1) = 1 AND NOT a.attnotnull
  AND n.nspname = current_schema()";
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *conn)
        .await
        .context("Failed to read nullable foreign keys")
}

async fn primary_key_postgres(conn: &mut sqlx::PgConnection, table: &str) -> Result<Vec<String>> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(
        "SELECT a.attname::text FROM pg_index i \
         JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) \
         WHERE i.indrelid = $1::regclass AND i.indisprimary",
    ))
    .bind(quote_ident(table))
    .fetch_all(&mut *conn)
    .await
    .with_context(|| format!("Failed to read the primary key of {}", table))
}

/// Foreign keys between tables of the current schema, as `(child, parent)` pairs.
pub(crate) async fn fk_edges_postgres(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<(String, String)>> {
    let sql = "SELECT c.relname::text, p.relname::text
FROM pg_constraint k
JOIN pg_class c ON c.oid = k.conrelid
JOIN pg_class p ON p.oid = k.confrelid
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE k.contype = 'f' AND n.nspname = current_schema()";

    sqlx::query_as::<_, (String, String)>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *conn)
        .await
        .context("Failed to read foreign keys")
}
