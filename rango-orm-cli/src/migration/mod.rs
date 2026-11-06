//! Migration engine: scan models → snapshot → diff (+ renames) → SQL.

pub mod diff;
pub mod renames;
pub mod scanner;
pub mod snapshot;
pub mod sql;
