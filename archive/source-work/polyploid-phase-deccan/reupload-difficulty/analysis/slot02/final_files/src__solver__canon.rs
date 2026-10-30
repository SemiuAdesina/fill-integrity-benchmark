
//! Canonical form and the order among distinct optima.
//!
//! Homolog labels are arbitrary, so `spec/result-format-v1.md` fixes one
//! relabelling of a reconstruction as the one to report: the four rows of each
//! parent in ascending order, ties between identical rows broken by the parent's
//! path signature and then by the permutation itself. It also fixes the order
//! used to pick between reconstructions that are not relabellings of each
//! other: parent A's rows, parent B's rows, then the whole path signature.

use super::tables::{SA, SB, STATE_INDEX};

/// All permutations of the four homolog indices, in ascending image order.
pub fn permutations() -> Vec<[u8; 4]> {
    let mut out = Vec::with_capacity(24);
    for a in 0..4u8 {
        for b in 0..4u8 {
            for c in 0..4u8 {
                for d in 0..4u8 {
                    let p = [a, b, c, d];
                    let mut seen = [false; 4];
                    if p.iter().all(|x| !std::mem::replace(&mut seen[usize::from(*x)], true)) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

/// Allele of homolog `h` at marker `m`.
#[inline]
pub fn allele(cols: &[u8], h: usize, m: usize) -> u8 {
    (cols[m] >> h) & 1
}

/// Reorders homologs so that the row at `h` moves to `p[h]`.
pub fn remap_cols(cols: &[u8], p: &[u8; 4]) -> Vec<u8> {
    cols.iter()
        .map(|column| {
            let mut out = 0u8;
            for h in 0usize..4 {
                if (column >> h) & 1 == 1 {
                    out |= 1u8 << p[h];
                }
            }
            out
        })
        .collect()
}

/// Rewrites every state of one parent through `p`.
pub fn remap_states(states: &[u8], p: &[u8; 4]) -> Vec<u8> {
    states
        .iter()
        .map(|s| {
            let i = usize::from(*s);
            STATE_INDEX[usize::from(p[usize::from(SA[i])])][usize::from(p[usize::from(SB[i])])]
        })
        .collect()
}

/// True when the four rows are already in ascending lexicographic order.
fn rows_ascending(cols: &[u8]) -> bool {
    let m = cols.len();
    for h in 1..4 {
        for k in 0..m {
            let above = allele(cols, h - 1, k);
            let below = allele(cols, h, k);
            if above < below {
                break;
            }
            if above > below {
                return false;
            }
        }
    }
    true
}

/// Permutations whose resulting rows are in ascending order.
pub fn sorting_permutations(cols: &[u8]) -> Vec<[u8; 4]> {
    permutations()
        .into_iter()
        .filter(|p| rows_ascending(&remap_cols(cols, p)))
        .collect()
}

/// The unique relabelling of a haplotype matrix whose rows are sorted.
///
/// Rows that are identical can be swapped without changing the matrix, so the
/// sorted form is the same whichever sorting permutation is used.
pub fn canon_cols(cols: &[u8]) -> Vec<u8> {
    match sorting_permutations(cols).first() {
        Some(p) => remap_cols(cols, p),
        None => cols.to_vec(),
    }
}

/// Flat path signature for one parent: progeny ascending, markers ascending.
pub fn signature(paths: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for states in paths {
        for s in states {
            out.push(SA[usize::from(*s)]);
            out.push(SB[usize::from(*s)]);
        }
    }
    out
}

/// The key `spec/result-format-v1.md` minimises between distinct optima.
pub fn distinct_key(cols_a: &[u8], cols_b: &[u8], pa: &[Vec<u8>], pb: &[Vec<u8>]) -> Vec<u8> {
    let mut key = Vec::new();
    for cols in [cols_a, cols_b] {
        for h in 0..4 {
            for column in cols {
                key.push((column >> h) & 1);
            }
        }
    }
    for c in 0..pa.len() {
        for s in &pa[c] {
            key.push(SA[usize::from(*s)]);
            key.push(SB[usize::from(*s)]);
        }
        for s in &pb[c] {
            key.push(SA[usize::from(*s)]);
            key.push(SB[usize::from(*s)]);
        }
    }
    key
}
