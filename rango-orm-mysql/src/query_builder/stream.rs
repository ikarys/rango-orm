use super::*;
use crate::row::MySqlRangoRow;
use futures_core::stream::BoxStream;
use futures_core::Stream;
use rango_orm_core::Error;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Rows of a query decoded one at a time as they arrive, from [`QueryBuilder::stream`].
/// Memory stays constant whatever the row count. Implements `futures_core::Stream`; the
/// inherent [`next`](Self::next) needs no combinator crate.
pub struct RowStream<'a, M> {
    rows: BoxStream<'a, sqlx::Result<MySqlRow>>,
    span: tracing::Span,
    seen: usize,
    _m: PhantomData<fn() -> M>,
}

impl<M: FromRow> RowStream<'_, M> {
    /// The next row, `None` after the last one.
    pub async fn next(&mut self) -> Option<Result<M>> {
        std::future::poll_fn(|cx| Pin::new(&mut *self).poll_next(cx)).await
    }
}

impl<M: FromRow> Stream for RowStream<'_, M> {
    type Item = Result<M>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let _entered = this.span.enter();
        let item = match std::task::ready!(this.rows.as_mut().poll_next(cx)) {
            Some(Ok(row)) => {
                this.seen += 1;
                Some(M::from_row_in(&MySqlRangoRow(row)).map_err(Error::from))
            }
            Some(Err(e)) => Some(Err(db_err(e))),
            None => {
                this.span.record("db.rows", this.seen);
                None
            }
        };
        Poll::Ready(item)
    }
}

impl<'a, M> QueryBuilder<'a, M>
where
    M: Model + ModelValues + FromRow,
{
    /// Run the query and decode the rows as they stream from the connection, for result
    /// sets too large for one `Vec`. No default row cap: `.limit()` is honoured, nothing
    /// else bounds the count. Relations are not loaded: `select_related` /
    /// `prefetch_related` are `Error::Invalid` here; use `.all()` for those.
    ///
    /// The stream borrows the pool or transaction the builder was made with.
    pub fn stream(mut self) -> Result<RowStream<'a, M>> {
        self.f.ensure_valid()?;
        if !self.f.select_related_specs().is_empty() || !self.f.prefetch_specs().is_empty() {
            return Err(Error::Invalid(
                "stream() does not load relations; use all() with select_related / prefetch_related"
                    .into(),
            ));
        }
        self.f = self.f.unlimited();
        let (sql, binds) = self.f.select_sql();
        let q = bind_sql_values(sqlx::query(sqlx::AssertSqlSafe(sql)), binds);
        let rows = match self.conn {
            Conn::Pool(p) => q.fetch(p),
            Conn::Tx(c) => q.fetch(c),
            Conn::Detached => {
                return Err(Error::Invalid(
                    "a sub-query builder has no connection and cannot be executed".into(),
                ));
            }
        };
        let span = tracing::info_span!(
            "db.query",
            db.system = "mysql",
            db.operation = "SELECT",
            db.sql.table = M::table_name(),
            db.rows = tracing::field::Empty,
            otel.kind = "client"
        );
        Ok(RowStream {
            rows,
            span,
            seen: 0,
            _m: PhantomData,
        })
    }
}
