
//! A per pair of progeny relaxation of the remaining markers.
//!
//! `RemainingBound` lets every progeny choose its own parental columns at every
//! undecided marker. That is where most of its slack comes from: two progeny
//! that want different alleles at a marker are both given what they want. This
//! bound takes the progeny two at a time and makes each pair share one column
//! pair per undecided marker, while still letting different pairs of progeny
//! disagree. It is a relaxation of the real problem, in which all progeny
//! share, so its value is an upper bound; and it is at least as tight as the
//! per progeny bound, because sharing within a pair is an extra constraint.
//!
//! The price is a joint state space of `100 x 100` per pair of progeny per
//! marker. The backward tables are built once, and at a node the bound costs
//! ten thousand additions per pair of progeny, so it is evaluated only for
//! nodes the cheaper bounds have not already discarded.

use crate::cross_input::Cross;

use super::progeny_dp::{StaticTerms, JOINT};
use super::warm_start::{node_term, Pair};

const JJ: usize = JOINT * JOINT;

pub struct PairBound {
    /// Progeny grouped two at a time; an odd last progeny stands alone and is
    /// covered by the per progeny bound instead.
    pub groups: Vec<(usize, usize)>,
    /// `suffix[g][m]`: bound on markers after `m` given the group's joint
    /// states `(j1, j2)` at `m`, flattened `j1 * JOINT + j2`.
    suffix: Vec<Vec<Vec<f64>>>,
    /// `rowmax[g][m][j1]`: largest suffix entry in row `j1`, so whole rows can
    /// be skipped when they cannot beat the running maximum.
    rowmax: Vec<Vec<Vec<f64>>>,
}

/// Best summed node term over the shared pairs, per joint state pair.
fn node_max_pair(cross: &Cross, terms: &StaticTerms, c1: usize, c2: usize, m: usize, pairs: &[Pair]) -> Vec<f64> {
    let mut out = vec![f64::NEG_INFINITY; JJ];
    let mut n1 = vec![f64::NEG_INFINITY; JOINT];
    let mut n2 = vec![f64::NEG_INFINITY; JOINT];
    let dr = terms.dr_at(m);
    for pair in pairs {
        let dose = terms.dose_row(&pair.0, &pair.1);
        let g1 = &cross.child_gl[c1][m];
        let g2 = &cross.child_gl[c2][m];
        for j in 0..JOINT {
            if dr[j] == f64::NEG_INFINITY {
                n1[j] = f64::NEG_INFINITY;
                n2[j] = f64::NEG_INFINITY;
            } else {
                n1[j] = g1[usize::from(dose[j])] + dr[j];
                n2[j] = g2[usize::from(dose[j])] + dr[j];
            }
        }
        for j1 in 0..JOINT {
            if n1[j1] == f64::NEG_INFINITY {
                continue;
            }
            let row = &mut out[j1 * JOINT..(j1 + 1) * JOINT];
            for j2 in 0..JOINT {
                let v = n1[j1] + n2[j2];
                if v > row[j2] {
                    row[j2] = v;
                }
            }
        }
    }
    out
}

/// Applies the factorised backward sweep on both progeny of a table.
fn backward_pair(terms: &StaticTerms, m: usize, future: &[f64], out: &mut [f64]) {
    // Progeny one along rows, progeny two along columns.
    let mut half = vec![f64::NEG_INFINITY; JJ];
    let mut col_in = vec![f64::NEG_INFINITY; JOINT];
    let mut col_out = vec![f64::NEG_INFINITY; JOINT];
    // Sweep progeny two (inner index) for each j1.
    for j1 in 0..JOINT {
        terms.advance_backward(m, &future[j1 * JOINT..(j1 + 1) * JOINT], &mut half[j1 * JOINT..(j1 + 1) * JOINT]);
    }
    // Sweep progeny one (outer index) for each j2.
    for j2 in 0..JOINT {
        for j1 in 0..JOINT {
            col_in[j1] = half[j1 * JOINT + j2];
        }
        terms.advance_backward(m, &col_in, &mut col_out);
        for j1 in 0..JOINT {
            out[j1 * JOINT + j2] = col_out[j1];
        }
    }
}

impl PairBound {
    pub fn build(cross: &Cross, terms: &StaticTerms, pairs: &[Vec<Pair>]) -> PairBound {
        let markers = cross.markers;
        let n = cross.offspring;
        let groups = choose_groups(cross, terms, pairs);
        let mut suffix = Vec::with_capacity(groups.len());
        let mut rowmax = Vec::with_capacity(groups.len());
        let mut prefix = Vec::with_capacity(groups.len());
        for &(c1, c2) in &groups {
            let node: Vec<Vec<f64>> = (0..markers).map(|m| node_max_pair(cross, terms, c1, c2, m, &pairs[m])).collect();
            let mut suf = vec![vec![0.0f64; JJ]; markers];
            let mut future = vec![f64::NEG_INFINITY; JJ];
            for m in (0..markers.saturating_sub(1)).rev() {
                for i in 0..JJ {
                    future[i] = node[m + 1][i] + suf[m + 1][i];
                }
                let mut carried = vec![f64::NEG_INFINITY; JJ];
                backward_pair(terms, m, &future, &mut carried);
                suf[m] = carried;
            }
            let rm: Vec<Vec<f64>> = suf
                .iter()
                .map(|table| (0..JOINT).map(|j1| table[j1 * JOINT..(j1 + 1) * JOINT].iter().cloned().fold(f64::NEG_INFINITY, f64::max)).collect())
                .collect();
            rowmax.push(rm);
            suffix.push(suf);
        }
        PairBound { groups, suffix, rowmax }
    }

    /// Upper bound on what the group can still earn after `marker`, given
    /// each progeny's exact arrival scores there.
    pub fn after(&self, g: usize, marker: usize, arrival1: &[f64], arrival2: &[f64]) -> f64 {
        let table = &self.suffix[g][marker];
        let rowmax = &self.rowmax[g][marker];
        let a2max = arrival2.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let mut best = f64::NEG_INFINITY;
        for j1 in 0..JOINT {
            let a1 = arrival1[j1];
            if a1 == f64::NEG_INFINITY || a1 + a2max + rowmax[j1] <= best {
                continue;
            }
            let row = &table[j1 * JOINT..(j1 + 1) * JOINT];
            for j2 in 0..JOINT {
                let a2 = arrival2[j2];
                if a2 == f64::NEG_INFINITY {
                    continue;
                }
                let v = a1 + a2 + row[j2];
                if v > best {
                    best = v;
                }
            }
        }
        best
    }

}
