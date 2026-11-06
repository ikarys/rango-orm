use crate::ops::DEFAULT_QUERY_LIMIT;
use crate::related::WithRelated;
use crate::row::{PgRangoRow, PgRangoRow2};
use rango_core::{ColumnRef, FromRow, Model, ModelValues, SqlValue, quote_ident};
use rango_core::{Error, Result};
use sqlx::Postgres;
use std::any::Any;
use std::collections::{HashMap, HashSet};
// ─── Bind helpers ─────────────────────────────────────────────────────────────
use sqlx::Row;

use super::relations::{extract_key_from_row, sql_value_key};
use super::*;

impl<M> QueryBuilder<M>
where
    M: Model + ModelValues + FromRow,
{
    // ── Terminal methods ──────────────────────────────────────────────────────
    /// Fetch all matching rows, populating relation caches when specs are present.
    ///
    /// Returns `Vec<WithRelated<M>>` — each item derefs to `M`, and related
    /// objects are accessible via `.related::<R>()` / `.prefetched::<R>()`.
    pub async fn all(self) -> Result<Vec<WithRelated<M>>> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();

        let mut sql = format!("SELECT * FROM {}", quote_ident(table));
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        if !self.order_by.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", self.order_by.join(", ")));
        }
        // Apply the default row cap when neither .limit() nor .unlimited() was called.
        if !self.explicit_limit {
            sql.push_str(&format!(" LIMIT {}", DEFAULT_QUERY_LIMIT));
        } else if let Some(l) = self.limit {
            sql.push_str(&format!(" LIMIT {}", l));
        }
        // explicit_limit=true, limit=None → .unlimited() was called; no LIMIT clause added
        if let Some(o) = self.offset {
            sql.push_str(&format!(" OFFSET {}", o));
        }

        // One connection for the main query and every relation query: one pool round-trip.
        let mut conn = self.pool.acquire().await.map_err(db_err)?;
        let rows = bind_and_fetch_all(&mut *conn, &sql, binds).await?;
        let mut results: Vec<WithRelated<M>> = Vec::with_capacity(rows.len());
        for row in rows {
            results.push(WithRelated::new(M::from_row(&PgRangoRow(row))?));
        }

        if results.is_empty() {
            return Ok(results);
        }

        // ── select_related: FK on M, bulk-fetch R by PK ───────────────────────
        for spec in &self.select_related {
            // Pair each result index with its FK value.
            let indexed_fks: Vec<(usize, SqlValue)> = results
                .iter()
                .enumerate()
                .filter_map(|(i, wr)| {
                    wr.inner
                        .field_values()
                        .into_iter()
                        .find(|(c, _)| *c == spec.fk_col.as_str())
                        .map(|(_, v)| (i, v))
                })
                .collect();

            if indexed_fks.is_empty() {
                continue;
            }

            // Deduplicate FK values while preserving order.
            let mut seen: HashSet<String> = HashSet::new();
            let mut unique_vals: Vec<SqlValue> = Vec::new();
            for (_, v) in &indexed_fks {
                if let Some(k) = sql_value_key(v)
                    && seen.insert(k)
                {
                    unique_vals.push(v.clone());
                }
            }
            if unique_vals.is_empty() {
                continue;
            }

            let placeholders: Vec<String> =
                (1..=unique_vals.len()).map(|i| format!("${}", i)).collect();
            let related_sql = format!(
                "SELECT * FROM {} WHERE {} IN ({})",
                quote_ident(spec.table_name),
                quote_ident(spec.pk_col),
                placeholders.join(", "),
            );

            let related_rows = bind_and_fetch_all(&mut *conn, &related_sql, unique_vals).await?;

            // Map pk key → PgRow (re-used for each M that shares the same FK).
            let mut row_by_pk: HashMap<String, sqlx::postgres::PgRow> = HashMap::new();
            for row in related_rows {
                if let Some(k) = extract_key_from_row(&row, spec.pk_col) {
                    row_by_pk.insert(k, row);
                }
            }

            for (i, fk_val) in &indexed_fks {
                if let Some(fk_key) = sql_value_key(fk_val)
                    && let Some(row) = row_by_pk.get(&fk_key)
                {
                    let boxed = (spec.from_row)(&PgRangoRow2::new(row)).map_err(|e| {
                        Error::Decode(format!("select_related({}): {}", spec.fk_col, e))
                    })?;
                    results[*i].insert_raw(spec.type_id, boxed);
                }
            }
        }

        // ── prefetch_related: FK on R, bulk-fetch and group by M PK ──────────
        for spec in &self.prefetch_related {
            let indexed_pks: Vec<(usize, SqlValue)> = results
                .iter()
                .enumerate()
                .map(|(i, wr)| (i, wr.inner.pk_value()))
                .collect();

            let mut seen: HashSet<String> = HashSet::new();
            let mut unique_pks: Vec<SqlValue> = Vec::new();
            for (_, v) in &indexed_pks {
                if let Some(k) = sql_value_key(v)
                    && seen.insert(k)
                {
                    unique_pks.push(v.clone());
                }
            }
            if unique_pks.is_empty() {
                continue;
            }

            let placeholders: Vec<String> =
                (1..=unique_pks.len()).map(|i| format!("${}", i)).collect();
            let prefetch_sql = format!(
                "SELECT * FROM {} WHERE {} IN ({})",
                quote_ident(spec.table_name),
                quote_ident(&spec.related_fk_col),
                placeholders.join(", "),
            );

            let prefetch_rows = bind_and_fetch_all(&mut *conn, &prefetch_sql, unique_pks).await?;

            // Group boxed R items by the FK value they carry.
            let mut grouped: HashMap<String, Vec<Box<dyn Any + Send + Sync>>> = HashMap::new();
            for row in &prefetch_rows {
                if let Some(fk_key) = extract_key_from_row(row, &spec.related_fk_col) {
                    let boxed = (spec.from_row)(&PgRangoRow2::new(row)).map_err(|e| {
                        Error::Decode(format!("prefetch_related({}): {}", spec.related_fk_col, e))
                    })?;
                    grouped.entry(fk_key).or_default().push(boxed);
                }
            }

            // Attach to each result — always set the cache (empty Vec for no matches).
            for (i, pk_val) in &indexed_pks {
                let items = sql_value_key(pk_val)
                    .and_then(|k| grouped.remove(&k))
                    .unwrap_or_default();
                let boxed_vec = (spec.collect_vec)(items);
                results[*i].insert_raw(spec.vec_type_id, boxed_vec);
            }
        }

        Ok(results)
    }

    /// Fetch the first matching row (no relation population).
    pub async fn one(self) -> Result<Option<WithRelated<M>>> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();

        let mut sql = format!("SELECT * FROM {}", quote_ident(table));
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        sql.push_str(" LIMIT 1");

        let rows = bind_and_fetch_all(&self.pool, &sql, binds).await?;
        match rows.into_iter().next() {
            Some(r) => Ok(Some(
                M::from_row(&PgRangoRow(r))
                    .map(WithRelated::new)
                    .map_err(Error::from)?,
            )),
            None => Ok(None),
        }
    }

    /// Count matching rows.
    pub async fn count(self) -> Result<i64> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();

        let mut sql = format!("SELECT COUNT(*) FROM {}", quote_ident(table));
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }

        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<i64, _>(0).unwrap_or(0))
    }

    /// Check if any matching row exists.
    pub async fn exists(self) -> Result<bool> {
        Ok(self.count().await? > 0)
    }

    /// SELECT only the specified columns — returns raw rows as `Vec<HashMap<String, SqlValue>>`.
    ///
    /// Use when you need a subset of columns without deserializing a full model.
    ///
    /// # Example
    /// ```rust,ignore
    /// let rows = User::filter(&pool)
    ///     .eq("active", true)
    ///     .values(&["id", "email"])
    ///     .await?;
    ///
    /// for row in rows {
    ///     println!("{:?}", row["email"]);
    /// }
    /// ```
    pub async fn values(
        self,
        cols: &[&str],
    ) -> Result<Vec<std::collections::HashMap<String, SqlValue>>> {
        use sqlx::Row;
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();

        let col_list = cols
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");

        let mut sql = format!("SELECT {} FROM {}", col_list, quote_ident(table));
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        if !self.order_by.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", self.order_by.join(", ")));
        }
        if !self.explicit_limit {
            sql.push_str(&format!(" LIMIT {}", DEFAULT_QUERY_LIMIT));
        } else if let Some(l) = self.limit {
            sql.push_str(&format!(" LIMIT {}", l));
        }
        if let Some(o) = self.offset {
            sql.push_str(&format!(" OFFSET {}", o));
        }

        let rows = bind_and_fetch_all(&self.pool, &sql, binds).await?;

        rows.into_iter()
            .map(|row| {
                use sqlx::{Column, TypeInfo};
                let mut map = std::collections::HashMap::new();
                for &col in cols {
                    let type_name = row.column(col).type_info().name().to_uppercase();
                    let val: SqlValue = match type_name.as_str() {
                        "BOOL" => row
                            .try_get::<Option<bool>, _>(col)
                            .ok()
                            .map(|v| v.map(SqlValue::Bool).unwrap_or(SqlValue::NullBool))
                            .unwrap_or(SqlValue::NullBool),
                        "INT2" => row
                            .try_get::<Option<i16>, _>(col)
                            .ok()
                            .map(|v| {
                                v.map(|n| SqlValue::BigInt(n as i64))
                                    .unwrap_or(SqlValue::NullBigInt)
                            })
                            .unwrap_or(SqlValue::NullBigInt),
                        "INT4" | "SERIAL" => row
                            .try_get::<Option<i32>, _>(col)
                            .ok()
                            .map(|v| {
                                v.map(|n| SqlValue::BigInt(n as i64))
                                    .unwrap_or(SqlValue::NullBigInt)
                            })
                            .unwrap_or(SqlValue::NullBigInt),
                        "INT8" | "BIGSERIAL" => row
                            .try_get::<Option<i64>, _>(col)
                            .ok()
                            .map(|v| v.map(SqlValue::BigInt).unwrap_or(SqlValue::NullBigInt))
                            .unwrap_or(SqlValue::NullBigInt),
                        "FLOAT4" => row
                            .try_get::<Option<f32>, _>(col)
                            .ok()
                            .map(|v| {
                                v.map(|n| SqlValue::Double(n as f64))
                                    .unwrap_or(SqlValue::NullDouble)
                            })
                            .unwrap_or(SqlValue::NullDouble),
                        "FLOAT8" | "NUMERIC" => row
                            .try_get::<Option<f64>, _>(col)
                            .ok()
                            .map(|v| v.map(SqlValue::Double).unwrap_or(SqlValue::NullDouble))
                            .unwrap_or(SqlValue::NullDouble),
                        "UUID" => row
                            .try_get::<Option<uuid::Uuid>, _>(col)
                            .ok()
                            .map(|v| v.map(SqlValue::Uuid).unwrap_or(SqlValue::NullUuid))
                            .unwrap_or(SqlValue::NullUuid),
                        _ => row
                            .try_get::<Option<String>, _>(col)
                            .ok()
                            .map(|v| v.map(SqlValue::Text).unwrap_or(SqlValue::NullText))
                            .unwrap_or(SqlValue::NullText),
                    };
                    map.insert(col.to_string(), val);
                }
                Ok(map)
            })
            .collect()
    }

    /// SELECT SUM(col) FROM ... WHERE ... — returns None if the table is empty or all values are NULL.
    pub async fn sum(self, col: impl ColumnRef<M>) -> Result<Option<f64>> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!(
            "SELECT SUM({})::float8 FROM {}",
            quote_ident(col.column_name()),
            quote_ident(table)
        );
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<Option<f64>, _>(0).unwrap_or(None))
    }

    /// SELECT AVG(col) FROM ... WHERE ... — returns None if the table is empty or all values are NULL.
    pub async fn avg(self, col: impl ColumnRef<M>) -> Result<Option<f64>> {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!(
            "SELECT AVG({})::float8 FROM {}",
            quote_ident(col.column_name()),
            quote_ident(table)
        );
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<Option<f64>, _>(0).unwrap_or(None))
    }

    /// SELECT MIN(col) FROM ... WHERE ...
    ///
    /// Generic over the return type — works for numbers, strings, dates, etc.
    /// Returns `None` if the table is empty or all values are NULL.
    ///
    /// # Example
    /// ```rust,ignore
    /// let oldest: Option<NaiveDate> = Event::filter(&pool).min::<NaiveDate>("date").await?;
    /// let lowest: Option<f64>       = Order::filter(&pool).min::<f64>("price").await?;
    /// ```
    pub async fn min<T>(self, col: impl ColumnRef<M>) -> Result<Option<T>>
    where
        T: for<'r> sqlx::Decode<'r, Postgres> + sqlx::Type<Postgres>,
    {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!(
            "SELECT MIN({}) FROM {}",
            quote_ident(col.column_name()),
            quote_ident(table)
        );
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<Option<T>, _>(0).unwrap_or(None))
    }

    /// SELECT MAX(col) FROM ... WHERE ...
    ///
    /// Generic over the return type — works for numbers, strings, dates, etc.
    /// Returns `None` if the table is empty or all values are NULL.
    ///
    /// # Example
    /// ```rust,ignore
    /// let latest: Option<DateTime<Utc>> = Post::filter(&pool).max::<DateTime<Utc>>("created_at").await?;
    /// let highest: Option<f64>          = Order::filter(&pool).max::<f64>("price").await?;
    /// ```
    pub async fn max<T>(self, col: impl ColumnRef<M>) -> Result<Option<T>>
    where
        T: for<'r> sqlx::Decode<'r, Postgres> + sqlx::Type<Postgres>,
    {
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!(
            "SELECT MAX({}) FROM {}",
            quote_ident(col.column_name()),
            quote_ident(table)
        );
        if !where_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&where_clause);
        }
        let row = bind_and_fetch_one(&self.pool, &sql, binds).await?;
        Ok(row.try_get::<Option<T>, _>(0).unwrap_or(None))
    }

    /// Execute a closure within a transaction, using this builder's pool.
    pub async fn transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: for<'c> FnOnce(
            &'c mut crate::transaction::RangoTransaction<'_>,
        ) -> crate::transaction::TxFuture<'c, T>,
    {
        crate::transaction::atomic(&self.pool, f).await
    }
}
