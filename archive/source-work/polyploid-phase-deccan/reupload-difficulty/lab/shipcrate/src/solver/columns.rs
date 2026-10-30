
//! Admissible allele columns and the incremental row order restriction.
//!
//! A "column" is one parent's four alleles at one marker. `parent_dosage` fixes
//! how many of the four are alternate, so the admissible columns at a marker are
//! exactly the four bit patterns of that popcount: one, four or six of them.
//!
//! Homolog labels are arbitrary, so every assignment belongs to an orbit of up
//! to twenty-four relabellings with an identical score. Canonical form requires
//! the four rows to be in ascending lexicographic order, and the search only
//! ever builds assignments that can still reach that order. That keeps one
//! representative per orbit instead of twenty-four.

use crate::cross_model::HOMOLOGS;

/// One parent's alleles at one marker, indexed by homolog.
pub type Column = [u8; HOMOLOGS];

/// Every column with the given number of alternate alleles, in ascending
/// numeric order of the packed bit pattern.
pub fn columns_for_dosage(dosage: u8) -> Vec<Column> {
    let mut out = Vec::new();
    for pattern in 0u8..16 {
        if pattern.count_ones() == u32::from(dosage) {
            let mut col = [0u8; HOMOLOGS];
            for h in 0..HOMOLOGS {
                col[h] = (pattern >> h) & 1;
            }
            out.push(col);
        }
    }
    out
}

/// Which adjacent row pairs are still equal across the markers chosen so far.
///
/// Bit `h` is set when row `h` and row `h + 1` have been equal at every marker
/// decided up to now. A pair that is still tied can be fixed by a later marker;
/// a pair that has already gone the wrong way cannot, which is what makes the
/// prune sound.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowOrder(u8);

impl RowOrder {
    /// Before any marker is chosen, all three adjacent pairs are equal.
    pub fn new() -> RowOrder {
        RowOrder(0b111)
    }

    /// Extends the order by one column.
    ///
    /// Returns `None` when the column puts a still tied pair out of ascending
    /// order, which no later marker can repair.
    pub fn extend(self, column: &Column) -> Option<RowOrder> {
        let mut tied = self.0;
        for h in 0..HOMOLOGS - 1 {
            let bit = 1u8 << h;
            if tied & bit == 0 {
                continue;
            }
            match column[h].cmp(&column[h + 1]) {
                std::cmp::Ordering::Greater => return None,
                std::cmp::Ordering::Less => tied &= !bit,
                std::cmp::Ordering::Equal => {}
            }
        }
        Some(RowOrder(tied))
    }

    /// True when rows `h` and `h + 1` are identical across every marker.
    ///
    /// These are the pairs that leave canonical form undetermined by row order
    /// alone, so the path signature has to settle them.
    #[cfg(test)]
    pub fn still_tied(self, h: usize) -> bool {
        self.0 & (1u8 << h) != 0
    }
}

impl Default for RowOrder {
    fn default() -> Self {
        RowOrder::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_counts_follow_the_binomial_coefficients() {
        let sizes: Vec<usize> = (0..=4).map(|d| columns_for_dosage(d).len()).collect();
        assert_eq!(sizes, vec![1, 4, 6, 4, 1]);
    }

    #[test]
    fn every_column_matches_its_dosage() {
        for d in 0..=4u8 {
            for col in columns_for_dosage(d) {
                assert_eq!(col.iter().sum::<u8>(), d);
            }
        }
    }

    #[test]
    fn row_order_rejects_a_descending_pair_and_frees_a_resolved_one() {
        let order = RowOrder::new();
        // Row 0 would hold the alternate allele while row 1 does not, which is
        // descending, so this column can never lead to canonical order.
        assert!(order.extend(&[1, 0, 0, 0]).is_none());

        // Resolving the first pair while leaving the others equal releases that
        // pair, after which a later descending column for it is acceptable.
        let resolved = order.extend(&[0, 1, 1, 1]).expect("ascending");
        assert!(!resolved.still_tied(0));
        assert!(resolved.still_tied(1) && resolved.still_tied(2));
        assert!(resolved.extend(&[1, 0, 0, 0]).is_some());
    }

    #[test]
    fn an_all_equal_column_keeps_every_pair_tied() {
        let order = RowOrder::new().extend(&[1, 1, 1, 1]).expect("equal is ascending");
        assert!((0..HOMOLOGS - 1).all(|h| order.still_tied(h)));
    }
}
