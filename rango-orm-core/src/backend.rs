use crate::config::BackendKind;

/// Core trait implemented by each database backend pool.
/// All Rango operations are generic over this trait.
pub trait RangoBackend: Send + Sync + Clone + 'static {
    fn backend_kind(&self) -> BackendKind;

    /// Returns the SQL placeholder for parameter N (1-indexed).
    /// Postgres → "$1", "$2" ...
    /// SQLite   → "?", "?" ...
    fn placeholder(&self, n: usize) -> String;
}
