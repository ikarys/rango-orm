//! Migration engine: scan models → snapshot → diff (+ renames) → SQL.

pub mod diff;
#[cfg(test)]
mod random_graphs;
pub mod renames;
pub mod scanner;
pub mod snapshot;
pub mod sql;
