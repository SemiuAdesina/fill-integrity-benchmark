
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
    /// Progeny grouped two at a time; an odd progeny stands alone and is
    /// covered by the per progeny bound instead.
    pub groups: Vec<(usize, usize)>,
    /// `suffix[g][m]`: bound on markers after `m` given the group's joint
    /// states `(j1, j2)` at `m`, flattened `j1 * JOINT + j2`.
    suffix: Vec<Vec<Vec<f64>>>,
    /// `prefix[g][m]`: bound on markers before `m` arriving in `(j1, j2)` at
    /// `m`, before `m`'s node terms. Used by the column filter.
    prefix: Vec<Vec<Vec<f64>>>,
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

fn forward_pair(terms: &StaticTerms, m: usize, prev: &[f64], out: &mut [f64]) {
    let mut half = vec![f64::NEG_INFINITY; JJ];
    let mut col_in = vec![f64::NEG_INFINITY; JOINT];
    let mut col_out = vec![f64::NEG_INFINITY; JOINT];
    for j1 in 0..JOINT {
        terms.advance(m, &prev[j1 * JOINT..(j1 + 1) * JOINT], &mut half[j1 * JOINT..(j1 + 1) * JOINT]);
    }
    for j2 in 0..JOINT {
        for j1 in 0..JOINT {
            col_in[j1] = half[j1 * JOINT + j2];
        }
        terms.advance(m, &col_in, &mut col_out);
        for j1 in 0..JOINT {
            out[j1 * JOINT + j2] = col_out[j1];
        }
    }
}

/// Pairs progeny that disagree most about the columns, greedily.
///
/// Under the per progeny relaxation each progeny has a set of column pairs it
/// is content with at each marker. Two progeny whose sets are disjoint at many
/// markers gain the most from being made to share, so the pairing that puts
/// the strongest disagreements together gives the tightest bound. Any pairing
/// is admissible; this one is just a better choice.
fn choose_groups_k(cross: &Cross, terms: &StaticTerms, pairs: &[Vec<Pair>], round: usize, banned: &[Vec<bool>]) -> Vec<(usize, usize)> {
    let n = cross.offspring;
    let markers = cross.markers;
    // Preferred pair sets per progeny per marker: pairs attaining the best
    // node term at some state on an optimal relaxed path.
    let mut prefs: Vec<Vec<Vec<bool>>> = Vec::with_capacity(n);
    for c in 0..n {
        let mut node = vec![vec![f64::NEG_INFINITY; JOINT]; markers];
        for m in 0..markers {
            for pair in &pairs[m] {
                for j in 0..JOINT {
                    let v = node_term(cross, terms, c, m, j, pair);
                    if v > node[m][j] {
                        node[m][j] = v;
                    }
                }
            }
        }
        let mut fwd = vec![vec![f64::NEG_INFINITY; JOINT]; markers];
        fwd[0].clone_from(&node[0]);
        for m in 1..markers {
            let mut out = vec![f64::NEG_INFINITY; JOINT];
            terms.advance(m - 1, &fwd[m - 1], &mut out);
            for j in 0..JOINT {
                fwd[m][j] = if out[j] == f64::NEG_INFINITY || node[m][j] == f64::NEG_INFINITY { f64::NEG_INFINITY } else { out[j] + node[m][j] };
            }
        }
        let mut post = vec![vec![0.0f64; JOINT]; markers];
        for m in (0..markers.saturating_sub(1)).rev() {
            let mut future = vec![f64::NEG_INFINITY; JOINT];
            for j in 0..JOINT {
                if node[m + 1][j] != f64::NEG_INFINITY && post[m + 1][j] != f64::NEG_INFINITY {
                    future[j] = node[m + 1][j] + post[m + 1][j];
                }
            }
            let mut carried = vec![f64::NEG_INFINITY; JOINT];
            terms.advance_backward(m, &future, &mut carried);
            post[m] = carried;
        }
        let best = fwd[markers - 1].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let mut pref_c = Vec::with_capacity(markers);
        for m in 0..markers {
            let mut set = vec![false; pairs[m].len()];
            for j in 0..JOINT {
                if fwd[m][j] == f64::NEG_INFINITY || post[m][j] == f64::NEG_INFINITY || fwd[m][j] + post[m][j] < best - 1e-9 {
                    continue;
                }
                for (k, pair) in pairs[m].iter().enumerate() {
                    if node_term(cross, terms, c, m, j, pair) >= node[m][j] - 1e-9 {
                        set[k] = true;
                    }
                }
            }
            pref_c.push(set);
        }
        prefs.push(pref_c);
    }
    let conflict = |a: usize, b: usize| -> usize {
        (0..markers).filter(|&m| !prefs[a][m].iter().zip(prefs[b][m].iter()).any(|(x, y)| *x && *y)).count()
    };
    let _ = round;
    let cap: usize = std::env::var("LAB_PAIR_GROUPS").ok().and_then(|v| v.parse().ok()).unwrap_or(n / 2);
    let mut used = vec![false; n];
    let mut groups = Vec::with_capacity(n / 2);
    // Greedy: repeatedly take the most conflicting unpaired pair; ties by index.
    loop {
        let mut best: Option<(usize, usize, usize)> = None;
        for a in 0..n {
            if used[a] { continue; }
            for b in (a + 1)..n {
                if used[b] || banned[a][b] { continue; }
                let w = conflict(a, b);
                if best.map_or(true, |(bw, _, _)| w > bw) {
                    best = Some((w, a, b));
                }
            }
        }
        let Some((_, a, b)) = best else { break };
        used[a] = true;
        used[b] = true;
        groups.push((a, b));
        if groups.len() >= cap { break; }
    }
    groups
}

/// Several disjoint pairings of the progeny, each giving its own relaxation.
///
/// Every pairing is a valid relaxation on its own, so the smallest bound any of
/// them gives is still an upper bound. Different pairings tighten different
/// disagreements, and later pairings are built with the earlier pairings' pairs
/// banned, so they see the disagreements the first one already covered.
pub struct PairBoundSet {
    pub rounds: Vec<PairBound>,
}

impl PairBoundSet {
    pub fn build(cross: &Cross, terms: &StaticTerms, pairs: &[Vec<Pair>], rounds: usize) -> PairBoundSet {
        let n = cross.offspring;
        let mut banned = vec![vec![false; n]; n];
        let mut out = Vec::with_capacity(rounds);
        for round in 0..rounds {
            let groups = choose_groups_k(cross, terms, pairs, round, &banned);
            if groups.is_empty() {
                break;
            }
            for &(a, b) in &groups {
                banned[a][b] = true;
                banned[b][a] = true;
            }
            out.push(PairBound::build_with(cross, terms, pairs, groups));
        }
        PairBoundSet { rounds: out }
    }
}

impl PairBound {
    pub fn build(cross: &Cross, terms: &StaticTerms, pairs: &[Vec<Pair>], with_prefix: bool) -> PairBound {
        let groups = choose_groups_k(cross, terms, pairs, 0, &vec![vec![false; cross.offspring]; cross.offspring]);
        PairBound::build_with(cross, terms, pairs, groups)
    }

    pub fn build_with(cross: &Cross, terms: &StaticTerms, pairs: &[Vec<Pair>], groups: Vec<(usize, usize)>) -> PairBound {
        let with_prefix = true;
        let markers = cross.markers;
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
            if with_prefix {
                let mut pre = vec![vec![0.0f64; JJ]; markers];
                let mut fwd = vec![f64::NEG_INFINITY; JJ];
                for i in 0..JJ {
                    fwd[i] = node[0][i];
                }
                for m in 1..markers {
                    let mut out = vec![f64::NEG_INFINITY; JJ];
                    forward_pair(terms, m - 1, &fwd, &mut out);
                    pre[m] = out.clone();
                    for i in 0..JJ {
                        fwd[i] = if out[i] == f64::NEG_INFINITY || node[m][i] == f64::NEG_INFINITY { f64::NEG_INFINITY } else { out[i] + node[m][i] };
                    }
                }
                prefix.push(pre);
            }
        }
        PairBound { groups, suffix, prefix, rowmax }
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

    /// Upper bound on the group's whole score if `pair` is used at `marker`.
    pub fn forced(&self, cross: &Cross, terms: &StaticTerms, g: usize, marker: usize, pair: &Pair) -> f64 {
        let (c1, c2) = self.groups[g];
        let pre = &self.prefix[g][marker];
        let suf = &self.suffix[g][marker];
        let mut best = f64::NEG_INFINITY;
        for j1 in 0..JOINT {
            let n1 = node_term(cross, terms, c1, marker, j1, pair);
            if n1 == f64::NEG_INFINITY {
                continue;
            }
            for j2 in 0..JOINT {
                let n2 = node_term(cross, terms, c2, marker, j2, pair);
                if n2 == f64::NEG_INFINITY {
                    continue;
                }
                let i = j1 * JOINT + j2;
                if pre[i] == f64::NEG_INFINITY || suf[i] == f64::NEG_INFINITY {
                    continue;
                }
                let v = pre[i] + n1 + n2 + suf[i];
                if v > best {
                    best = v;
                }
            }
        }
        best
    }
}
