//! pgvector support shared by the derive, the migration CLI and the Postgres backend.

use crate::column::{Column, ColumnRef};
use crate::fields::FieldVector;

/// pgvector distance: the search operator, and the operator class of an HNSW index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distance {
    /// Euclidean distance, `<->`.
    L2,
    /// Cosine distance (`1 - cosine similarity`, 0 to 2), `<=>`.
    Cosine,
    /// Negative inner product, `<#>`: smaller is closer.
    InnerProduct,
}

impl Distance {
    /// Every distance, in the order `name` lists them in error messages.
    pub const ALL: [Distance; 3] = [Distance::L2, Distance::Cosine, Distance::InnerProduct];

    /// The name used in `#[field(index = "...")]` and in migration snapshots.
    pub fn name(self) -> &'static str {
        match self {
            Distance::L2 => "l2",
            Distance::Cosine => "cosine",
            Distance::InnerProduct => "inner_product",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.name() == name)
    }

    /// The SQL operator.
    pub fn op(self) -> &'static str {
        match self {
            Distance::L2 => "<->",
            Distance::Cosine => "<=>",
            Distance::InnerProduct => "<#>",
        }
    }

    /// The operator class an HNSW index needs for this operator to use it.
    pub fn index_ops(self) -> &'static str {
        match self {
            Distance::L2 => "vector_l2_ops",
            Distance::Cosine => "vector_cosine_ops",
            Distance::InnerProduct => "vector_ip_ops",
        }
    }
}

/// A vector column of model `M` with `N` dimensions: what `nearest` and `within` accept,
/// so a non-vector column or a query of another dimension does not compile.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a vector column of `{M}`",
    note = "pass a `FieldVector<N>` column constant such as `Doc::EMBEDDING`, and a query `&FieldVector<N>` of the same `N`"
)]
pub trait VectorColumn<M, const N: usize>: ColumnRef<M> {}

impl<M, const N: usize> VectorColumn<M, N> for Column<M, FieldVector<N>> {}
impl<M, const N: usize> VectorColumn<M, N> for Column<M, Option<FieldVector<N>>> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for d in Distance::ALL {
            assert_eq!(Distance::from_name(d.name()), Some(d));
        }
        assert_eq!(Distance::from_name("Cosine"), None);
    }
}
