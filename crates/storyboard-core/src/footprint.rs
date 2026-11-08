//! Precise names for values an operation can change.

use std::collections::BTreeSet;

/// Returns whether two footprint names could write the same stored value.
/// A value itself and any of its descendants overlap; siblings do not.
pub fn overlaps(left: &str, right: &str) -> bool {
    left == "project"
        || right == "project"
        || left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

/// Returns true when no footprint in either collection overlaps another.
pub fn disjoint(left: &[String], right: &[String]) -> bool {
    !left
        .iter()
        .any(|left| right.iter().any(|right| overlaps(left, right)))
}

pub(crate) fn sorted(values: impl IntoIterator<Item = String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
