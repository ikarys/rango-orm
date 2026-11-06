use crate::ops::DEFAULT_QUERY_LIMIT;
use crate::related::WithRelated;
use crate::row::{PgPrefixedRow, PgRangoRow, PgRangoRow2};
use rango_orm_core::{ColumnRef, FromRow, Model, ModelValues, RangoRow, SqlValue, quote_ident};
use rango_orm_core::{Error, Result};
use sqlx::Postgres;
use std::any::Any;
use std::collections::{HashMap, HashSet};
// ─── Bind helpers ─────────────────────────────────────────────────────────────
use sqlx::Row;

use super::*;
use rango_orm_core::related::{row_key, sql_value_key};

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
        let (sql, binds) = self.select_sql();

        // One connection for the main query and every prefetch query: one pool round-trip.
        let mut conn = self.pool.acquire().await.map_err(db_err)?;
        let rows = bind_and_fetch_all(&mut *conn, &sql, binds).await?;
        let mut results: Vec<WithRelated<M>> = Vec::with_capacity(rows.len());
        if self.select_related.is_empty() {
            for row in rows {
                results.push(WithRelated::new(M::from_row(&PgRangoRow(row))?));
            }
        } else {
            // ── select_related: R's columns came back in the same row (LEFT JOIN) ──
            for row in &rows {
                let mut wr = WithRelated::new(M::from_row(&PgRangoRow2::new(row))?);
                for (i, spec) in self.select_related.iter().enumerate() {
                    let r_row = PgPrefixedRow::new(row, format!("__r{i}_"));
                    // NULL primary key: no matching R (NULL or dangling FK).
                    if r_row.is_null(spec.pk_col) {
                        continue;
                    }
                    let boxed = (spec.from_row)(&r_row).map_err(|e| {
                        Error::Decode(format!("select_related({}): {}", spec.fk_col, e))
                    })?;
                    wr.insert_raw(spec.type_id, boxed);
                }
                results.push(wr);
            }
        }

        if results.is_empty() {
            return Ok(results);
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

            // `= ANY($1)`: one array parameter, so the statement text (and its cached
            // plan) is the same whatever the number of keys, and there is no
            // 65 535-parameter cap.
            let prefetch_sql = format!("{} = ANY($1)", spec.select);
            let like = unique_pks[0].clone();
            let prefetch_rows = fetch_by_keys(&mut *conn, &prefetch_sql, unique_pks).await?;

            // Group boxed R items by the FK value they carry.
            let mut grouped: HashMap<String, Vec<Box<dyn Any + Send + Sync>>> = HashMap::new();
            for row in &prefetch_rows {
                let r_row = PgRangoRow2::new(row);
                if let Some(fk_key) = row_key(&r_row, &spec.key_col, &like) {
                    let boxed = (spec.from_row)(&r_row).map_err(|e| {
                        Error::Decode(format!("prefetch_related({}): {}", spec.key_col, e))
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
