
//! Removing column pairs that no optimal assignment can use.
//!
//! Given an incumbent, a column pair at a marker can be discarded outright if
//! the best any assignment using it could score, under a relaxation, is below
//! the incumbent. The relaxation here is the per progeny one the bounds use —
//! every other marker's columns chosen freely per progeny, paths continuous —
//! with the tested pair held fixed at its marker. That is an upper bound on
//! every completion through the pair, so removing it loses no optimum and no
//! tie: a pair is removed only when its bound is strictly below the incumbent
//! by more than the tie tolerance.
//!
//! Removing pairs tightens the relaxation at every other marker, so the pass
//! repeats until nothing more is removed. The search then branches only over
//! what survives, and both bounds are built over the reduced sets.

use crate::cross_input::Cross;

use super::progeny_dp::{StaticTerms, JOINT};
use super::prices::Prices;
use super::warm_start::{node_term, Pair};
use super::window_bound::WindowBound;
use super::pair_bound::PairBound;

const TIE_TOLERANCE: f64 = 1e-9;

/// What the pass did, for the tractability record.
#[derive(Clone, Copy, Default, Debug)]
pub struct FilterStats {
    pub pairs_before: u64,
    pub pairs_after: u64,
    pub passes: u32,
}

/// Best node term at each marker and state over the pairs still allowed.
fn node_max(cross: &Cross, terms: &StaticTerms, c: usize, pairs: &[Vec<Pair>], lam_c: &[Vec<f64>]) -> Vec<Vec<f64>> {
    (0..cross.markers)
        .map(|m| {
            let mut row = vec![f64::NEG_INFINITY; JOINT];
            for j in 0..JOINT {
                for (k, pair) in pairs[m].iter().enumerate() {
                    let v = node_term(cross, terms, c, m, j, pair);
                    if v == f64::NEG_INFINITY {
                        continue;
                    }
                    let v = v + lam_c[m][k];
                    if v > row[j] {
                        row[j] = v;
                    }
                }
            }
            row
        })
        .collect()
}

/// Reduces `pairs` in place against `incumbent`; returns what happened.
pub fn reduce(
    cross: &Cross,
    terms: &StaticTerms,
    pairs: &mut [Vec<Pair>],
    prices: &mut Prices,
    incumbent: f64,
) -> FilterStats {
    let use_window = std::env::var("LAB_NO_WINDOW").is_err();
    let markers = cross.markers;
    let mut stats = FilterStats {
        pairs_before: pairs.iter().map(|p| p.len() as u64).sum(),
        ..FilterStats::default()
    };
    loop {
        stats.passes += 1;
        // Relaxed prefix (before the node term) and suffix per progeny.
        let mut pre: Vec<Vec<Vec<f64>>> = Vec::with_capacity(cross.offspring);
        let mut post: Vec<Vec<Vec<f64>>> = Vec::with_capacity(cross.offspring);
        for c in 0..cross.offspring {
            let nm = node_max(cross, terms, c, pairs, &prices.lam[c]);
            let mut fwd = vec![vec![f64::NEG_INFINITY; JOINT]; markers];
            let mut pre_c = vec![vec![0.0f64; JOINT]; markers];
            for m in 0..markers {
                if m > 0 {
                    let mut out = vec![f64::NEG_INFINITY; JOINT];
                    terms.advance(m - 1, &fwd[m - 1], &mut out);
                    pre_c[m] = out;
                }
                for j in 0..JOINT {
                    fwd[m][j] =
                        if nm[m][j] == f64::NEG_INFINITY || pre_c[m][j] == f64::NEG_INFINITY {
                            f64::NEG_INFINITY
                        } else {
                            pre_c[m][j] + nm[m][j]
                        };
                }
            }
            let mut post_c = vec![vec![0.0f64; JOINT]; markers];
            for m in (0..markers.saturating_sub(1)).rev() {
                let mut future = vec![f64::NEG_INFINITY; JOINT];
                for j in 0..JOINT {
                    if nm[m + 1][j] != f64::NEG_INFINITY && post_c[m + 1][j] != f64::NEG_INFINITY {
                        future[j] = nm[m + 1][j] + post_c[m + 1][j];
                    }
                }
                let mut carried = vec![f64::NEG_INFINITY; JOINT];
                terms.advance_backward(m, &future, &mut carried);
                post_c[m] = carried;
            }
            pre.push(pre_c);
            post.push(post_c);
        }
        let window = if use_window { Some(WindowBound::build(cross, terms, pairs)) } else { None };
        let pair_bound = if std::env::var("LAB_NO_PAIR").is_err() { Some(PairBound::build(cross, terms, pairs, true)) } else { None };
        let mut removed = 0usize;
        for m in 0..markers {
            let keep: Vec<bool> = pairs[m]
                .iter()
                .enumerate()
                .map(|(k, pair)| {
                    let mut bound = 0.0;
                    for c in 0..cross.offspring {
                        let mut best = f64::NEG_INFINITY;
                        for j in 0..JOINT {
                            if pre[c][m][j] == f64::NEG_INFINITY || post[c][m][j] == f64::NEG_INFINITY {
                                continue;
                            }
                            let node = node_term(cross, terms, c, m, j, pair);
                            if node == f64::NEG_INFINITY {
                                continue;
                            }
                            // Prices at the forced marker sum to zero over
                            // progeny, so adding them keeps the total exact
                            // for this pair while the relaxed prefix and
                            // suffix remain priced upper bounds.
                            let v = pre[c][m][j] + node + prices.lam[c][m][k] + post[c][m][j];
                            if v > best {
                                best = v;
                            }
                        }
                        if best == f64::NEG_INFINITY {
                            return false;
                        }
                        bound += best;
                    }
                    if let Some(wb) = &window {
                        bound = bound.min(wb.forced(m, k));
                    }
                    if let Some(pb) = &pair_bound {
                        let mut paired = 0.0;
                        for g in 0..pb.groups.len() {
                            paired += pb.forced(cross, terms, g, m, pair);
                        }
                        if cross.offspring % 2 == 1 {
                            let c = cross.offspring - 1;
                            let mut best = f64::NEG_INFINITY;
                            for j in 0..JOINT {
                                if pre[c][m][j] == f64::NEG_INFINITY || post[c][m][j] == f64::NEG_INFINITY {
                                    continue;
                                }
                                let node = node_term(cross, terms, c, m, j, pair);
                                if node == f64::NEG_INFINITY {
                                    continue;
                                }
                                let v = pre[c][m][j] + node + prices.lam[c][m][k] + post[c][m][j];
                                if v > best {
                                    best = v;
                                }
                            }
                            paired += best;
                        }
                        bound = bound.min(paired);
                    }
                    bound >= incumbent - TIE_TOLERANCE
                })
                .collect();
            let before = pairs[m].len();
            let mut k = 0;
            pairs[m].retain(|_| { let keep_it = keep[k]; k += 1; keep_it });
            for c in 0..cross.offspring {
                let mut k = 0;
                prices.lam[c][m].retain(|_| { let keep_it = keep[k]; k += 1; keep_it });
            }
            removed += before - pairs[m].len();
            // The incumbent itself is admissible and ties itself, so a marker
            // never empties.
            debug_assert!(!pairs[m].is_empty());
        }
        if removed == 0 {
            break;
        }
    }
    stats.pairs_after = pairs.iter().map(|p| p.len() as u64).sum();
    stats
}
