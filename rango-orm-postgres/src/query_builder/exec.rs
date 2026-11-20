use crate::related::WithRelated;
use crate::row::{PgPrefixedRow, PgRangoRow, PgRangoRow2, decode_all};
use rango_orm_core::{ColumnRef, FromRow, Model, ModelValues, Partial, RangoRow, SqlValue};
use rango_orm_core::{Error, Result};
use sqlx::Postgres;
use std::any::Any;
use std::collections::{HashMap, HashSet};
// ─── Bind helpers ─────────────────────────────────────────────────────────────
use sqlx::Row;

use super::*;
use rango_orm_core::related::{row_key, sql_value_key};

impl<'a, M> QueryBuilder<'a, M>
where
    M: Model + ModelValues + FromRow,
{
    // ── Terminal methods ──────────────────────────────────────────────────────
    /// Fetch all matching rows, populating relation caches when specs are present.
    ///
    /// Returns `Vec<WithRelated<M>>` — each item derefs to `M`, and related
    /// objects are accessible via `.related::<R>()` / `.prefetched::<R>()`.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn all(mut self) -> Result<Vec<WithRelated<M>>> {
        self.f.ensure_valid()?;
        let (sql, binds) = self.f.select_sql();

        // One connection for the main query and every prefetch query: one pool round-trip.
        let mut source = std::mem::take(&mut self.conn);
        let mut conn = source.acquire().await?;
        let rows = bind_and_fetch_all(&mut *conn, &sql, binds).await?;
        self.warn_if_capped(rows.len());
        tracing::Span::current().record("db.rows", rows.len());
        let mut results: Vec<WithRelated<M>> = Vec::with_capacity(rows.len());
        if self.f.select_related_specs().is_empty() {
            for row in rows {
                results.push(WithRelated::new(M::from_row(&PgRangoRow(row))?));
            }
        } else {
            // ── select_related: R's columns came back in the same row (LEFT JOIN) ──
            for row in &rows {
                let mut wr = WithRelated::new(M::from_row(&PgRangoRow2::new(row))?);
                for (i, spec) in self.f.select_related_specs().iter().enumerate() {
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
        for spec in self.f.prefetch_specs() {
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
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn one(mut self) -> Result<Option<M>> {
        self.f.ensure_valid()?;
        // Same SQL as `all()` (filters, ordering, offset), capped at one row.
        self = self.limit(1);
        let (sql, binds) = self.f.select_sql();

        let rows = self.conn.fetch_all(&sql, binds).await?;
        match rows.into_iter().next() {
            Some(r) => Ok(Some(M::from_row(&PgRangoRow(r)).map_err(Error::from)?)),
            None => Ok(None),
        }
    }

    /// `UPDATE .. SET .. WHERE ..` on every matching row, from the `.set(col, value)` pairs.
    /// Returns the number of rows affected. Model hooks are not fired (same as Django).
    ///
    /// ```rust,ignore
    /// let n = User::filter(&pool).lt(User::AGE, 18).set(User::ACTIVE, false).update().await?;
    /// ```
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "UPDATE", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn update(mut self) -> Result<u64> {
        let (sql, binds) = self.f.update_sql()?;
        let rows = self.conn.execute(&sql, binds).await?;
        tracing::Span::current().record("db.rows", rows);
        Ok(rows)
    }

    /// `DELETE FROM .. WHERE ..` on every matching row (all rows when there is no filter).
    /// Returns the number of rows affected. Model hooks are not fired (same as Django);
    /// `ON DELETE` actions run in the database.
    ///
    /// ```rust,ignore
    /// let n = Session::filter(&pool).lt(Session::EXPIRES_AT, now).delete().await?;
    /// ```
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "DELETE", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn delete(mut self) -> Result<u64> {
        let (sql, binds) = self.f.delete_sql()?;
        let rows = self.conn.execute(&sql, binds).await?;
        tracing::Span::current().record("db.rows", rows);
        Ok(rows)
    }

    /// Count matching rows.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn count(mut self) -> Result<i64> {
        self.f.ensure_valid()?;
        let (sql, binds) = self.f.count_sql();

        let row = self.conn.fetch_one(&sql, binds).await?;
        Ok(row.try_get::<i64, _>(0).unwrap_or(0))
    }

    /// Check if any matching row exists.
    pub async fn exists(mut self) -> Result<bool> {
        self.f.ensure_valid()?;
        use sqlx::Row;
        let (sql, binds) = self.f.exists_sql();
        let row = self.conn.fetch_one(&sql, binds).await?;
        row.try_get::<bool, _>(0).map_err(crate::error::db_err)
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
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn values(
        mut self,
        cols: &[&str],
    ) -> Result<Vec<std::collections::HashMap<String, SqlValue>>> {
        self.f.ensure_valid()?;
        for c in cols {
            if let Some(msg) = rango_orm_core::unknown_column::<M>(c) {
                return Err(Error::Invalid(msg));
            }
        }
        let (sql, binds) = self.f.values_sql(cols);

        let rows = self.conn.fetch_all(&sql, binds).await?;
        self.warn_if_capped(rows.len());

        rows.iter().map(|row| row_to_map(row, cols)).collect()
    }

    /// One `SELECT` with `GROUP BY` / `HAVING` built from `group_by()`, `annotate_*()` and
    /// `having()`: one map per group (a single row when there is no `group_by`), keyed by
    /// the group columns and the annotation aliases. `COUNT` is `SqlValue::BigInt`,
    /// `SUM` / `AVG` are `SqlValue::Double`, `MIN` / `MAX` keep the column type.
    ///
    /// ```rust,ignore
    /// let rows = Order::filter(&pool)
    ///     .group_by(Order::COUNTRY)
    ///     .annotate_count("n")
    ///     .annotate_sum("total", Order::AMOUNT)
    ///     .having("total", Cmp::Gt, 100.0)
    ///     .order_by("-total")
    ///     .aggregate()
    ///     .await?;
    /// ```
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn aggregate(mut self) -> Result<Vec<std::collections::HashMap<String, SqlValue>>> {
        let (sql, binds) = self.f.group_sql()?;
        let rows = self.conn.fetch_all(&sql, binds).await?;
        self.warn_if_capped(rows.len());
        tracing::Span::current().record("db.rows", rows.len());
        let keys = self.f.aggregate_keys();
        let names: Vec<&str> = keys.iter().map(|(k, _)| k.as_str()).collect();
        rows.iter().map(|row| row_to_map(row, &names)).collect()
    }

    /// SELECT SUM(col) FROM ... WHERE ... — returns None if the table is empty or all values are NULL.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn sum(mut self, col: impl ColumnRef<M>) -> Result<Option<f64>> {
        self.f.ensure_valid()?;
        if let Some(msg) = rango_orm_core::unknown_column::<M>(col.column_name()) {
            return Err(Error::Invalid(msg));
        }
        let (sql, binds) = self.f.aggregate_sql("SUM", col.column_name());
        let row = self.conn.fetch_one(&sql, binds).await?;
        Ok(row.try_get::<Option<f64>, _>(0).unwrap_or(None))
    }

    /// SELECT AVG(col) FROM ... WHERE ... — returns None if the table is empty or all values are NULL.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn avg(mut self, col: impl ColumnRef<M>) -> Result<Option<f64>> {
        self.f.ensure_valid()?;
        if let Some(msg) = rango_orm_core::unknown_column::<M>(col.column_name()) {
            return Err(Error::Invalid(msg));
        }
        let (sql, binds) = self.f.aggregate_sql("AVG", col.column_name());
        let row = self.conn.fetch_one(&sql, binds).await?;
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
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn min<T>(mut self, col: impl ColumnRef<M>) -> Result<Option<T>>
    where
        T: for<'r> sqlx::Decode<'r, Postgres> + sqlx::Type<Postgres>,
    {
        self.f.ensure_valid()?;
        if let Some(msg) = rango_orm_core::unknown_column::<M>(col.column_name()) {
            return Err(Error::Invalid(msg));
        }
        let (sql, binds) = self.f.aggregate_sql("MIN", col.column_name());
        let row = self.conn.fetch_one(&sql, binds).await?;
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
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn max<T>(mut self, col: impl ColumnRef<M>) -> Result<Option<T>>
    where
        T: for<'r> sqlx::Decode<'r, Postgres> + sqlx::Type<Postgres>,
    {
        self.f.ensure_valid()?;
        if let Some(msg) = rango_orm_core::unknown_column::<M>(col.column_name()) {
            return Err(Error::Invalid(msg));
        }
        let (sql, binds) = self.f.aggregate_sql("MAX", col.column_name());
        let row = self.conn.fetch_one(&sql, binds).await?;
        Ok(row.try_get::<Option<T>, _>(0).unwrap_or(None))
    }
}

impl<'a, M> QueryBuilder<'a, M>
where
    M: Model + ModelValues + FromRow,
{
    /// Select only the columns of the `#[derive(Partial)]` struct `P`.
    ///
    /// Same filters, ordering and limits as `.all()`; relations are not supported.
    pub fn select_as<P: Partial<Model = M>>(self) -> SelectAs<'a, M, P> {
        SelectAs {
            qb: self,
            _phantom: std::marker::PhantomData,
        }
    }
}

/// A query selecting a partial struct, built by `QueryBuilder::select_as`.
pub struct SelectAs<'a, M, P> {
    qb: QueryBuilder<'a, M>,
    _phantom: std::marker::PhantomData<P>,
}

impl<M, P> SelectAs<'_, M, P>
where
    M: Model + ModelValues + FromRow,
    P: Partial<Model = M>,
{
    fn cols_sql() -> String {
        let cols: Vec<String> = P::columns()
            .iter()
            .map(|c| rango_orm_core::quote_ident(c))
            .collect();
        cols.join(", ")
    }

    /// Returns the SQL that would be executed by `.all()`, for debugging.
    pub fn explain(&self) -> String {
        self.qb.f.select_sql_cols(&Self::cols_sql()).0
    }

    /// Fetch all matching rows as `P`.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn all(mut self) -> Result<Vec<P>> {
        self.qb.f.ensure_valid()?;
        if !self.qb.f.select_related_specs().is_empty() || !self.qb.f.prefetch_specs().is_empty() {
            return Err(Error::Invalid(
                "select_related / prefetch_related are not supported with select_as".into(),
            ));
        }
        let (sql, binds) = self.qb.f.select_sql_cols(&Self::cols_sql());
        let rows = self.qb.conn.fetch_all(&sql, binds).await?;
        self.qb.warn_if_capped(rows.len());
        tracing::Span::current().record("db.rows", rows.len());
        decode_all::<P>(rows)
    }

    /// Fetch the first matching row as `P`.
    pub async fn one(self) -> Result<Option<P>> {
        let rows = self.qb.limit(1).select_as::<P>().all().await?;
        Ok(rows.into_iter().next())
    }
}

impl<'a, M> QueryBuilder<'a, M>
where
    M: Model + ModelValues + FromRow,
{
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "postgresql", db.operation = "EXPLAIN", db.sql.table = M::table_name(), otel.kind = "client"))]
    /// Runs the `.all()` query under `EXPLAIN ANALYZE` and returns the plan with timings.
    /// The query is executed (its rows are discarded): wrap writes-in-functions in a rolled-back transaction.
    pub async fn explain_analyze(&mut self) -> Result<String> {
        self.f.ensure_valid()?;
        let (sql, binds) = self.f.select_sql();
        let rows = self
            .conn
            .fetch_all(&format!("EXPLAIN ANALYZE {sql}"), binds)
            .await?;
        let lines = rows
            .iter()
            .map(|r| {
                r.try_get::<String, _>(0)
                    .map_err(|e| Error::Decode(e.to_string()))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(lines.join("\n"))
    }
}

/// One row of `values()` / `aggregate()` as `key → SqlValue`, decoded from the driver type.
fn row_to_map(
    row: &sqlx::postgres::PgRow,
    cols: &[&str],
) -> Result<std::collections::HashMap<String, SqlValue>> {
    use sqlx::{Column, Row, TypeInfo};
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
            "NUMERIC" => row
                .try_get::<Option<rango_orm_core::Decimal>, _>(col)
                .ok()
                .map(|v| v.map(SqlValue::Decimal).unwrap_or(SqlValue::NullDecimal))
                .unwrap_or(SqlValue::NullDecimal),
            "FLOAT8" => row
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
}
