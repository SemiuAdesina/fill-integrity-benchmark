
//! Fragments, bands and the matching rule. **This module defines what it means
//! for a map to reproduce a gel.**
//!
//! A digest cuts the construct at the union of its enzymes' positions; two
//! enzymes cutting at the same position make one cut. The fragments are the
//! circular gaps between consecutive cuts, and an uncut circle is one fragment
//! of the whole length. Fragments shorter than the detection limit do not
//! reach the gel. A predicted band list is the ascending fragment lengths at or
//! above the limit; an observed band carries a length and a multiplicity, the
//! number of fragments that ran together.
//!
//! Matching pairs the two lists in ascending order after expanding observed
//! bands by multiplicity. The counts must agree and every pair must lie within
//! `ceil(error_percent * observed / 100)` of each other. The deviation of a
//! match is the sum over pairs of the squared difference.

use std::collections::BTreeMap;

use crate::input::Band;

/// Circular gaps between consecutive cut positions on a circle of `n` bases.
/// `positions` must be ascending without duplicates; an empty list gives the
/// single uncut fragment.
pub fn fragments(positions: &[usize], n: usize) -> Vec<usize> {
    if positions.is_empty() {
        return vec![n];
    }
    let mut out: Vec<usize> = positions.windows(2).map(|w| w[1] - w[0]).collect();
    out.push(n - positions[positions.len() - 1] + positions[0]);
    out
}

/// Union of the named enzymes' positions, ascending, each position once.
pub fn union(cuts: &BTreeMap<String, Vec<usize>>, enzymes: &[String]) -> Vec<usize> {
    let mut all: Vec<usize> = enzymes
        .iter()
        .flat_map(|e| cuts.get(e).map(|v| v.iter().copied()).into_iter().flatten())
        .collect();
    all.sort_unstable();
    all.dedup();
    all
}

/// Ascending lengths of the fragments at or above `detection_limit`.
pub fn visible(fragments: &[usize], detection_limit: u32) -> Vec<u32> {
    let mut out: Vec<u32> = fragments
        .iter()
        .filter(|&&f| f >= detection_limit as usize)
        .map(|&f| f as u32)
        .collect();
    out.sort_unstable();
    out
}

/// The predicted band list of a digest over the given cut positions.
pub fn predicted(cuts: &BTreeMap<String, Vec<usize>>, enzymes: &[String], n: usize, detection_limit: u32) -> Vec<u32> {
    visible(&fragments(&union(cuts, enzymes), n), detection_limit)
}

/// Groups ascending lengths into bands with multiplicities.
pub fn group(lengths: &[u32]) -> Vec<Band> {
    let mut out: Vec<Band> = Vec::new();
    let mut sorted = lengths.to_vec();
    sorted.sort_unstable();
    for length in sorted {
        match out.last_mut() {
            Some(last) if last.length == length => last.multiplicity += 1,
            _ => out.push(Band { length, multiplicity: 1 }),
        }
    }
    out
}

/// Expands bands by multiplicity into ascending lengths.
pub fn expand(bands: &[Band]) -> Vec<u32> {
    let mut out = Vec::new();
    for band in bands {
        for _ in 0..band.multiplicity {
            out.push(band.length);
        }
    }
    out.sort_unstable();
    out
}

/// `ceil(error_percent * observed / 100)`.
pub fn tolerance(observed: u32, error_percent: u32) -> u32 {
    let numerator = u64::from(error_percent) * u64::from(observed) + 99;
    (numerator / 100) as u32
}

/// The deviation of a match, or `None` when the lists do not match.
pub fn deviation(predicted: &[u32], bands: &[Band], error_percent: u32) -> Option<u64> {
    let observed = expand(bands);
    if observed.len() != predicted.len() {
        return None;
    }
    let mut sorted = predicted.to_vec();
    sorted.sort_unstable();
    let mut total = 0u64;
    for (&p, &o) in sorted.iter().zip(observed.iter()) {
        let gap = if p > o { p - o } else { o - p };
        if gap > tolerance(o, error_percent) {
            return None;
        }
        total += u64::from(gap) * u64::from(gap);
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circular_gaps_and_the_uncut_circle() {
        assert_eq!(fragments(&[], 100), vec![100]);
        assert_eq!(fragments(&[10], 100), vec![100]);
        assert_eq!(fragments(&[10, 40, 70], 100), vec![30, 30, 40]);
    }

    #[test]
    fn coincident_cuts_merge() {
        let mut cuts = BTreeMap::new();
        cuts.insert("A".to_string(), vec![10, 50]);
        cuts.insert("B".to_string(), vec![50, 80]);
        assert_eq!(union(&cuts, &["A".to_string(), "B".to_string()]), vec![10, 50, 80]);
        assert_eq!(predicted(&cuts, &["A".to_string(), "B".to_string()], 100, 1), vec![30, 30, 40]);
    }

    #[test]
    fn tolerance_rounds_up_and_matching_is_positional() {
        assert_eq!(tolerance(1000, 2), 20);
        assert_eq!(tolerance(1001, 2), 21);
        assert_eq!(tolerance(50, 1), 1);
        let bands = vec![Band { length: 100, multiplicity: 2 }, Band { length: 300, multiplicity: 1 }];
        assert_eq!(deviation(&[99, 101, 302], &bands, 1), Some(6));
        assert_eq!(deviation(&[99, 101], &bands, 1), None);
        assert_eq!(deviation(&[97, 101, 302], &bands, 1), None);
        assert_eq!(group(&[302, 100, 100]), vec![Band { length: 100, multiplicity: 2 }, Band { length: 302, multiplicity: 1 }]);
    }
}
