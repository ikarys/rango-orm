//! Where a `QueryBuilder` runs: a pool or a borrowed connection / transaction.

use super::{bind_and_fetch_all, bind_and_fetch_one};
use crate::error::db_err;
use rango_orm_core::{Error, Result, SqlValue};
use sqlx::pool::PoolConnection;
use sqlx::sqlite::SqliteRow;
use sqlx::{Sqlite, SqliteConnection, SqlitePool, Transaction};
use std::ops::{Deref, DerefMut};

/// The connection source of a `QueryBuilder`.
///
/// Build it with `.into()` from `&SqlitePool`, `&mut SqliteConnection` or `&mut Transaction`:
/// `User::filter(&pool)` outside a transaction, `User::filter(&mut **tx)` inside
/// `rango::atomic`.
#[derive(Default)]
pub enum Conn<'a> {
    Pool(&'a SqlitePool),
    Tx(&'a mut SqliteConnection),
    /// Sub-builders (`group`, `where_exists`, ...): they only carry filters, never run.
    #[default]
    Detached,
}

impl<'a> From<&'a SqlitePool> for Conn<'a> {
    fn from(pool: &'a SqlitePool) -> Self {
        Conn::Pool(pool)
    }
}

impl<'a> From<&'a mut SqliteConnection> for Conn<'a> {
    fn from(conn: &'a mut SqliteConnection) -> Self {
        Conn::Tx(conn)
    }
}

impl<'a> From<&'a mut Transaction<'_, Sqlite>> for Conn<'a> {
    fn from(tx: &'a mut Transaction<'_, Sqlite>) -> Self {
        Conn::Tx(tx)
    }
}

/// A connection checked out from a [`Conn`] for one or several statements.
pub(super) enum ConnRef<'c> {
    Pool(PoolConnection<Sqlite>),
    Tx(&'c mut SqliteConnection),
}

impl Deref for ConnRef<'_> {
    type Target = SqliteConnection;
    fn deref(&self) -> &SqliteConnection {
        match self {
            ConnRef::Pool(c) => c,
            ConnRef::Tx(c) => c,
        }
    }
}

impl DerefMut for ConnRef<'_> {
    fn deref_mut(&mut self) -> &mut SqliteConnection {
        match self {
            ConnRef::Pool(c) => c,
            ConnRef::Tx(c) => c,
        }
    }
}

impl Conn<'_> {
    pub(super) async fn acquire(&mut self) -> Result<ConnRef<'_>> {
        match self {
            Conn::Pool(p) => Ok(ConnRef::Pool(p.acquire().await.map_err(db_err)?)),
            Conn::Tx(c) => Ok(ConnRef::Tx(c)),
            Conn::Detached => Err(Error::Invalid(
                "a sub-query builder has no connection and cannot be executed".into(),
            )),
        }
    }

    pub(super) async fn fetch_all(
        &mut self,
        sql: &str,
        binds: Vec<SqlValue>,
    ) -> Result<Vec<SqliteRow>> {
        let mut c = self.acquire().await?;
        bind_and_fetch_all(&mut *c, sql, binds).await
    }

    pub(super) async fn fetch_one(&mut self, sql: &str, binds: Vec<SqlValue>) -> Result<SqliteRow> {
        let mut c = self.acquire().await?;
        bind_and_fetch_one(&mut *c, sql, binds).await
    }
}
