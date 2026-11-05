use crate::row::SqliteRangoRow;
use rango_core::{quote_ident, ColumnRef, FromRow, Model, ModelValues, SqlValue};
use rango_core::{Error, Result};
// ─── Helpers ─────────────────────────────────────────────────────────────────

use super::*;

impl<M> QueryBuilder<M>
where
    M: Model + ModelValues + FromRow,
{
    pub async fn all(self) -> Result<Vec<M>> {
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
            .map(|r| M::from_row(&SqliteRangoRow(r)).map_err(Error::from))
            .collect()
    }

    pub async fn one(self) -> Result<Option<M>> {
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
            Some(r) => Ok(Some(M::from_row(&SqliteRangoRow(r)).map_err(Error::from)?)),
            None => Ok(None),
        }
    }

    pub async fn count(self) -> Result<i64> {
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

    pub async fn exists(self) -> Result<bool> {
        Ok(self.count().await? > 0)
    }

    /// SELECT only the specified columns — returns raw rows as `Vec<HashMap<String, SqlValue>>`.
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

    pub async fn sum(self, col: impl ColumnRef<M>) -> Result<Option<f64>> {
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

    pub async fn avg(self, col: impl ColumnRef<M>) -> Result<Option<f64>> {
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

    pub async fn min<T>(self, col: impl ColumnRef<M>) -> Result<Option<T>>
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
    {
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

    pub async fn max<T>(self, col: impl ColumnRef<M>) -> Result<Option<T>>
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
    {
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
