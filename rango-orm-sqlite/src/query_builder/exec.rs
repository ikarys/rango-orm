use crate::error::db_err;
use crate::row::{decode_all, SqlitePrefixedRow, SqliteRangoRow, SqliteRowRef};
use rango_orm_core::related::{row_key, sql_value_key};
use rango_orm_core::{
    quote_ident, ColumnRef, Error, FromRow, Model, ModelValues, Partial, RangoRow, Result,
    SqlValue, WithRelated,
};
use std::any::Any;
use std::collections::{HashMap, HashSet};

use super::*;

const PREFETCH_CHUNK: usize = 900; // stays under SQLite's old 999-parameter limit.

impl<M> QueryBuilder<M>
where
    M: Model + ModelValues + FromRow,
{
    /// Fetch all matching rows, populating relation caches when specs are present.
    ///
    /// Returns `Vec<WithRelated<M>>` — each item derefs to `M`, and related
    /// objects are accessible via `.related::<R>()` / `.prefetched::<R>()`.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn all(self) -> Result<Vec<WithRelated<M>>> {
        self.ensure_valid()?;
        let (sql, binds) = self.select_sql();

        // One connection for the main query and every prefetch query.
        let mut conn = self.pool.acquire().await.map_err(db_err)?;
        let rows = bind_and_fetch_all(&mut *conn, &sql, binds).await?;
        tracing::Span::current().record("db.rows", rows.len());
        let mut results: Vec<WithRelated<M>> = Vec::with_capacity(rows.len());

        if self.select_related.is_empty() {
            let models = decode_all::<M>(rows)?;
            results.extend(models.into_iter().map(WithRelated::new));
        } else {
            // ── select_related: R's columns came back in the same row (LEFT JOIN) ──
            for row in &rows {
                let mut wr = WithRelated::new(M::from_row(&SqliteRowRef(row))?);
                for (i, spec) in self.select_related.iter().enumerate() {
                    let r_row = SqlitePrefixedRow::new(row, format!("__r{i}_"));
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
                if let Some(k) = sql_value_key(v) {
                    if seen.insert(k) {
                        unique_pks.push(v.clone());
                    }
                }
            }
            if unique_pks.is_empty() {
                continue;
            }

            let like = unique_pks[0].clone();
            let mut grouped: HashMap<String, Vec<Box<dyn Any + Send + Sync>>> = HashMap::new();

            for chunk in unique_pks.chunks(PREFETCH_CHUNK) {
                let placeholders = vec!["?"; chunk.len()].join(", ");
                let prefetch_sql = format!("{} IN ({})", spec.select, placeholders);
                let prefetch_rows =
                    bind_and_fetch_all(&mut *conn, &prefetch_sql, chunk.to_vec()).await?;

                // Group boxed R items by the FK value they carry.
                for row in &prefetch_rows {
                    let r_row = SqliteRowRef(row);
                    if let Some(fk_key) = row_key(&r_row, &spec.key_col, &like) {
                        let boxed = (spec.from_row)(&r_row).map_err(|e| {
                            Error::Decode(format!("prefetch_related({}): {}", spec.key_col, e))
                        })?;
                        grouped.entry(fk_key).or_default().push(boxed);
                    }
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
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn one(mut self) -> Result<Option<M>> {
        self.ensure_valid()?;
        // Same SQL as `all()` (filters, ordering, offset), capped at one row.
        self.limit = Some(1);
        self.explicit_limit = true;
        let (sql, binds) = self.select_sql();

        let rows = bind_and_fetch_all(&self.pool, &sql, binds).await?;
        match rows.into_iter().next() {
            Some(r) => Ok(Some(M::from_row(&SqliteRangoRow(r)).map_err(Error::from)?)),
            None => Ok(None),
        }
    }

    /// Count matching rows.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn count(self) -> Result<i64> {
        self.ensure_valid()?;
        use sqlx::Row;
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
        self.ensure_valid()?;
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
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn values(
        self,
        cols: &[&str],
    ) -> Result<Vec<std::collections::HashMap<String, SqlValue>>> {
        self.ensure_valid()?;
        for c in cols {
            if let Some(msg) = rango_orm_core::unknown_column::<M>(c) {
                return Err(Error::Invalid(msg));
            }
        }
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
        let clauses = self.order_clauses();
        if !clauses.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", clauses.join(", ")));
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
                let mut map = std::collections::HashMap::new();
                for &col in cols {
                    let val: SqlValue = if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(col) {
                        SqlValue::BigInt(v)
                    } else if let Ok(Some(v)) = row.try_get::<Option<f64>, _>(col) {
                        SqlValue::Double(v)
                    } else if let Ok(Some(v)) = row.try_get::<Option<String>, _>(col) {
                        SqlValue::Text(v)
                    } else {
                        SqlValue::NullText
                    };
                    map.insert(col.to_string(), val);
                }
                Ok(map)
            })
            .collect()
    }

    /// SELECT SUM(col) FROM ... WHERE ... — returns None if the table is empty or all values are NULL.
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn sum(self, col: impl ColumnRef<M>) -> Result<Option<f64>> {
        self.ensure_valid()?;
        if let Some(msg) = rango_orm_core::unknown_column::<M>(col.column_name()) {
            return Err(Error::Invalid(msg));
        }
        use sqlx::Row;
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!(
            "SELECT CAST(SUM({}) AS REAL) FROM {}",
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
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn avg(self, col: impl ColumnRef<M>) -> Result<Option<f64>> {
        self.ensure_valid()?;
        if let Some(msg) = rango_orm_core::unknown_column::<M>(col.column_name()) {
            return Err(Error::Invalid(msg));
        }
        use sqlx::Row;
        let table = M::table_name();
        let (where_clause, binds) = self.build_where();
        let mut sql = format!(
            "SELECT AVG({}) FROM {}",
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
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn min<T>(self, col: impl ColumnRef<M>) -> Result<Option<T>>
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
    {
        self.ensure_valid()?;
        if let Some(msg) = rango_orm_core::unknown_column::<M>(col.column_name()) {
            return Err(Error::Invalid(msg));
        }
        use sqlx::Row;
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
    #[tracing::instrument(name = "db.query", skip_all, fields(db.system = "sqlite", db.operation = "SELECT", db.sql.table = M::table_name(), db.rows = tracing::field::Empty, otel.kind = "client"))]
    pub async fn max<T>(self, col: impl ColumnRef<M>) -> Result<Option<T>>
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
    {
        self.ensure_valid()?;
        if let Some(msg) = rango_orm_core::unknown_column::<M>(col.column_name()) {
            return Err(Error::Invalid(msg));
        }
        use sqlx::Row;
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
}

impl<M> QueryBuilder<M>
where
    M: Model + ModelValues + FromRow,
{
    /// Select only the columns of the `#[derive(Partial)]` struct `P`.
    ///
    /// Same filters, ordering and limits as `.all()`; relations are not supported.
    pub fn select_as<P: Partial<Model = M>>(self) -> SelectAs<M, P> {
        SelectAs {
            qb: self,
            _phantom: std::marker::PhantomData,
        }
    }
}

/// A query selecting a partial struct, built by `QueryBuilder::select_as`.
pub struct SelectAs<M, P> {
    qb: QueryBuilder<M>,
    _phantom: std::marker::PhantomData<P>,
}

impl<M, P> SelectAs<M, P>
where
    M: Model + ModelValues + FromRow,
    P: Partial<Model = M>,
{
    /// Fetch all matching rows as `P`.
    pub async fn all(self) -> Result<Vec<P>> {
        self.qb.ensure_valid()?;
        if !self.qb.select_related.is_empty() || !self.qb.prefetch_related.is_empty() {
            return Err(Error::Invalid(
                "select_related / prefetch_related are not supported with select_as".into(),
            ));
        }
        let cols: Vec<String> = P::columns().iter().map(|c| quote_ident(c)).collect();
        let (sql, binds) = self.qb.select_sql_cols(&cols.join(", "));
        let rows = bind_and_fetch_all(&self.qb.pool, &sql, binds).await?;
        decode_all::<P>(rows)
    }

    /// Fetch the first matching row as `P`.
    pub async fn one(self) -> Result<Option<P>> {
        let rows = self.qb.limit(1).select_as::<P>().all().await?;
        Ok(rows.into_iter().next())
    }
}
