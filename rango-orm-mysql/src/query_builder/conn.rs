//! Where a `QueryBuilder` runs: a pool or a borrowed connection / transaction.

use super::{bind_and_fetch_all, bind_and_fetch_one};
use crate::error::db_err;
use rango_orm_core::{Error, Result, SqlValue};
use sqlx::mysql::MySqlRow;
use sqlx::pool::PoolConnection;
use sqlx::{MySql, MySqlConnection, MySqlPool, Transaction};
use std::ops::{Deref, DerefMut};

/// The connection source of a `QueryBuilder`.
///
/// Build it with `.into()` from `&MySqlPool`, `&mut MySqlConnection` or `&mut Transaction`:
/// `User::filter(&pool)` outside a transaction, `User::filter(&mut **tx)` inside
/// `rango::atomic`.
#[derive(Default)]
pub enum Conn<'a> {
    Pool(&'a MySqlPool),
    Tx(&'a mut MySqlConnection),
    /// Sub-builders (`group`, `where_exists`, ...): they only carry filters, never run.
    #[default]
    Detached,
}

impl<'a> From<&'a MySqlPool> for Conn<'a> {
    fn from(pool: &'a MySqlPool) -> Self {
        Conn::Pool(pool)
    }
}

impl<'a> From<&'a mut MySqlConnection> for Conn<'a> {
    fn from(conn: &'a mut MySqlConnection) -> Self {
        Conn::Tx(conn)
    }
}

impl<'a> From<&'a mut Transaction<'_, MySql>> for Conn<'a> {
    fn from(tx: &'a mut Transaction<'_, MySql>) -> Self {
        Conn::Tx(tx)
    }
}

/// A connection checked out from a [`Conn`] for one or several statements.
pub(super) enum ConnRef<'c> {
    Pool(PoolConnection<MySql>),
    Tx(&'c mut MySqlConnection),
}

impl Deref for ConnRef<'_> {
    type Target = MySqlConnection;
    fn deref(&self) -> &MySqlConnection {
        match self {
            ConnRef::Pool(c) => c,
            ConnRef::Tx(c) => c,
        }
    }
}

impl DerefMut for ConnRef<'_> {
    fn deref_mut(&mut self) -> &mut MySqlConnection {
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
    ) -> Result<Vec<MySqlRow>> {
        let mut c = self.acquire().await?;
        bind_and_fetch_all(&mut *c, sql, binds).await
    }

    pub(super) async fn fetch_one(&mut self, sql: &str, binds: Vec<SqlValue>) -> Result<MySqlRow> {
        let mut c = self.acquire().await?;
        bind_and_fetch_one(&mut *c, sql, binds).await
    }
}
