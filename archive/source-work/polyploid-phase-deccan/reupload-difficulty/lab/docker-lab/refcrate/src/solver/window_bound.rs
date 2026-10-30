
//! A bound that keeps the columns shared and relaxes path continuity only
//! between adjacent intervals.
//!
//! `MarkerBound` frees the paths entirely: every interval is credited its best
//! transition and every marker its best state. This bound is the next step up.
//! It runs a dynamic programme over the shared column pair per marker, and for
//! each interval charges every progeny the best it can do over a *consistent
//! pair of states* at the interval's two ends, transition included. Each
//! marker's node term is split in half between the two intervals that touch it
//! (the first and last markers give their whole term to their only interval),
//! so summing over intervals counts every node term exactly once for any real
//! path. What is relaxed is continuity from one interval to the next: a
//! progeny may leave interval `m` in one state and enter interval `m + 1` in
//! another. Every real reconstruction is feasible for the relaxed problem at
//! the same value, so the maximum of the relaxed problem is an upper bound.

use crate::cross_input::Cross;

use super::progeny_dp::{StaticTerms, JOINT};
use super::warm_start::{node_term, Pair};

pub struct WindowBound {
    /// `suffix[m][k]`: bound on intervals `m..` with pair `k` at marker `m`,
    /// counting `m`'s node term at its interval weight.
    suffix: Vec<Vec<f64>>,
    /// `prefix[m][k]`: bound on intervals `..m` with pair `k` at marker `m`,
    /// counting `m`'s node term at its interval weight.
    prefix: Vec<Vec<f64>>,
    /// `remaining[m]`: bound on everything after marker `m` once every term
    /// through `m` is exact, whichever pair was chosen at `m`.
    remaining: Vec<f64>,
}

impl WindowBound {
    pub fn build(cross: &Cross, terms: &StaticTerms, pairs: &[Vec<Pair>]) -> WindowBound {
        let markers = cross.markers;
        let n = cross.offspring;
        let intervals = markers.saturating_sub(1);
        let weight_left = |m: usize| if m == 0 { 1.0 } else { 0.5 };
        let weight_right = |m: usize| if m + 1 == markers - 1 { 1.0 } else { 0.5 };

        // Per interval m, per left pair k, per right pair l: summed over progeny.
        // Also the "left weight zero" variant used for `remaining`.
        let mut w = vec![Vec::new(); intervals];
        let mut w_zero_left = vec![Vec::new(); intervals];
        let mut left_node = vec![f64::NEG_INFINITY; JOINT];
        let mut carried = vec![f64::NEG_INFINITY; JOINT];
        for m in 0..intervals {
            let nl = pairs[m].len();
            let nr = pairs[m + 1].len();
            w[m] = vec![0.0f64; nl * nr];
            w_zero_left[m] = vec![0.0f64; nr];
            let wl = weight_left(m);
            let wr = weight_right(m);
            for c in 0..n {
                // Zero-left variant: j free at m (legal states only).
                for j in 0..JOINT {
                    left_node[j] = if terms.dr_at(m)[j] == f64::NEG_INFINITY { f64::NEG_INFINITY } else { 0.0 };
                }
                terms.advance(m, &left_node, &mut carried);
                for (l, q) in pairs[m + 1].iter().enumerate() {
                    let mut best = f64::NEG_INFINITY;
                    for jn in 0..JOINT {
                        let node = node_term(cross, terms, c, m + 1, jn, q);
                        if node == f64::NEG_INFINITY || carried[jn] == f64::NEG_INFINITY {
                            continue;
                        }
                        let v = carried[jn] + wr * node;
                        if v > best {
                            best = v;
                        }
                    }
                    w_zero_left[m][l] += best;
                }
                for (k, p) in pairs[m].iter().enumerate() {
                    for j in 0..JOINT {
                        let node = node_term(cross, terms, c, m, j, p);
                        left_node[j] = if node == f64::NEG_INFINITY { f64::NEG_INFINITY } else { wl * node };
                    }
                    terms.advance(m, &left_node, &mut carried);
                    for (l, q) in pairs[m + 1].iter().enumerate() {
                        let mut best = f64::NEG_INFINITY;
                        for jn in 0..JOINT {
                            let node = node_term(cross, terms, c, m + 1, jn, q);
                            if node == f64::NEG_INFINITY || carried[jn] == f64::NEG_INFINITY {
                                continue;
                            }
                            let v = carried[jn] + wr * node;
                            if v > best {
                                best = v;
                            }
                        }
                        w[m][k * nr + l] += best;
                    }
                }
            }
        }

        // Suffix over pairs.
        let mut suffix = vec![Vec::new(); markers];
        suffix[markers - 1] = vec![0.0f64; pairs[markers - 1].len()];
        for m in (0..intervals).rev() {
            let nr = pairs[m + 1].len();
            suffix[m] = (0..pairs[m].len())
                .map(|k| {
                    let mut best = f64::NEG_INFINITY;
                    for l in 0..nr {
                        let v = w[m][k * nr + l] + suffix[m + 1][l];
                        if v > best {
                            best = v;
                        }
                    }
                    best
                })
                .collect();
        }
        // Prefix over pairs.
        let mut prefix = vec![Vec::new(); markers];
        prefix[0] = vec![0.0f64; pairs[0].len()];
        for m in 0..intervals {
            let nr = pairs[m + 1].len();
            let nl = pairs[m].len();
            prefix[m + 1] = (0..nr)
                .map(|l| {
                    let mut best = f64::NEG_INFINITY;
                    for k in 0..nl {
                        let v = prefix[m][k] + w[m][k * nr + l];
                        if v > best {
                            best = v;
                        }
                    }
                    best
                })
                .collect();
        }
        // Remaining after marker m with everything through m exact: the first
        // interval leaves m's state free and charges nothing for m's node.
        let mut remaining = vec![0.0f64; markers];
        for m in 0..intervals {
            let mut best = f64::NEG_INFINITY;
            for l in 0..pairs[m + 1].len() {
                let v = w_zero_left[m][l] + suffix[m + 1][l];
                if v > best {
                    best = v;
                }
            }
            remaining[m] = best;
        }
        WindowBound { suffix, prefix, remaining }
    }

    /// Upper bound on everything after `marker`, once the prefix is exact.
    pub fn after(&self, marker: usize) -> f64 {
        self.remaining[marker]
    }

    /// Upper bound on the whole cross if pair `k` is used at `marker`.
    pub fn forced(&self, marker: usize, k: usize) -> f64 {
        self.prefix[marker][k] + self.suffix[marker][k]
    }

    /// Upper bound on the whole cross.
    pub fn root(&self) -> f64 {
        self.suffix[0].iter().cloned().fold(f64::NEG_INFINITY, f64::max)
    }
}
