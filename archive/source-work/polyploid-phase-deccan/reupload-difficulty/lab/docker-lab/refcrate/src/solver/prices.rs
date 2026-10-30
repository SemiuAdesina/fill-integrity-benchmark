
//! Lagrangian prices that tighten the per progeny relaxation.
//!
//! The per progeny bound lets every progeny choose its own column pair at
//! every marker. The real problem makes them share one. Prices restore some of
//! that coupling without giving up the per progeny decomposition: each progeny
//! `c` receives an additive price `lambda[c][m][k]` on pair `k` at marker `m`,
//! and the prices are constrained to sum to zero over progeny for every `(m,
//! k)`. Any shared assignment then collects a total price of exactly zero, so
//! the priced relaxation is still an upper bound on every real reconstruction,
//! whatever the prices are. Choosing them to make the bound as small as
//! possible is a convex minimisation, done here by a projected subgradient
//! method with Polyak steps against the incumbent.
//!
//! The prices are computed once at the root and reused at every node: the
//! bound for the undecided markers is `sum over c` of a priced Viterbi from
//! the node's arrival scores, and the price sum still cancels over any shared
//! completion of those markers.

use crate::cross_input::Cross;

use super::progeny_dp::{StaticTerms, JOINT};
use super::warm_start::{node_term, Pair};

/// Prices indexed `[progeny][marker][pair index]`, and the bound they give.
pub struct Prices {
    pub lam: Vec<Vec<Vec<f64>>>,
    pub bound: f64,
    pub iterations: u32,
}

impl Prices {
    pub fn zero(cross: &Cross, pairs: &[Vec<Pair>]) -> Prices {
        Prices {
            lam: (0..cross.offspring)
                .map(|_| pairs.iter().map(|p| vec![0.0f64; p.len()]).collect())
                .collect(),
            bound: f64::INFINITY,
            iterations: 0,
        }
    }
}

/// Priced Viterbi for one progeny: its relaxed value and the pair index it
/// chose at every marker along one optimal path.
fn priced_viterbi(
    cross: &Cross,
    terms: &StaticTerms,
    pairs: &[Vec<Pair>],
    lam_c: &[Vec<f64>],
    c: usize,
    weights: &mut [Vec<f64>],
) -> f64 {
    let markers = cross.markers;
    // node[m][j] and the pair achieving it
    let mut node = vec![vec![f64::NEG_INFINITY; JOINT]; markers];
    let mut arg = vec![vec![0usize; JOINT]; markers];
    for m in 0..markers {
        for (k, pair) in pairs[m].iter().enumerate() {
            let price = lam_c[m][k];
            for j in 0..JOINT {
                let v = node_term(cross, terms, c, m, j, pair);
                if v == f64::NEG_INFINITY {
                    continue;
                }
                let v = v + price;
                if v > node[m][j] {
                    node[m][j] = v;
                    arg[m][j] = k;
                }
            }
        }
    }
    let mut fwd = vec![vec![f64::NEG_INFINITY; JOINT]; markers];
    for j in 0..JOINT {
        fwd[0][j] = node[0][j];
    }
    for m in 1..markers {
        let mut out = vec![f64::NEG_INFINITY; JOINT];
        terms.advance(m - 1, &fwd[m - 1], &mut out);
        for j in 0..JOINT {
            fwd[m][j] = if out[j] == f64::NEG_INFINITY || node[m][j] == f64::NEG_INFINITY {
                f64::NEG_INFINITY
            } else {
                out[j] + node[m][j]
            };
        }
    }
    // Best future strictly after m, per state at m.
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
    // Spread the choice indicator uniformly over every (state, pair) that lies
    // on some optimal relaxed path at each marker. The relaxed problem is
    // highly degenerate — relabelling homologs, and several pairs giving one
    // dosage, leave many tied optima — and a single arbitrary optimum is a poor
    // subgradient. The bound itself is exact whatever direction is used.
    for m in 0..markers {
        for k in 0..pairs[m].len() {
            weights[m][k] = 0.0;
        }
        let mut on_path: Vec<usize> = Vec::new();
        for j in 0..JOINT {
            if fwd[m][j] != f64::NEG_INFINITY && post[m][j] != f64::NEG_INFINITY && fwd[m][j] + post[m][j] >= best - 1e-9 {
                on_path.push(j);
            }
        }
        let share = 1.0 / on_path.len() as f64;
        for &j in &on_path {
            let target = node[m][j];
            let mut tied: Vec<usize> = Vec::new();
            for (k, pair) in pairs[m].iter().enumerate() {
                let v = node_term(cross, terms, c, m, j, pair);
                if v != f64::NEG_INFINITY && v + lam_c[m][k] >= target - 1e-9 {
                    tied.push(k);
                }
            }
            for &k in &tied {
                weights[m][k] += share / tied.len() as f64;
            }
        }
        let _ = arg[m][0];
    }
    best
}

/// Minimises the priced bound by projected subgradient descent.
///
/// `lower` is a score some admissible assignment attains, so the bound can
/// never legitimately fall below it; the Polyak step uses that distance.
pub fn optimise(
    cross: &Cross,
    terms: &StaticTerms,
    pairs: &[Vec<Pair>],
    lower: f64,
    max_iterations: u32,
) -> Prices {
    let n = cross.offspring;
    let markers = cross.markers;
    let mut current = Prices::zero(cross, pairs);
    let mut best_lam = current.lam.clone();
    let mut best_bound = f64::INFINITY;
    let mut beta = 1.0f64;
    let mut stall = 0u32;
    let mut weights: Vec<Vec<Vec<f64>>> = (0..n)
        .map(|_| pairs.iter().map(|p| vec![0.0f64; p.len()]).collect())
        .collect();
    let trace = std::env::var("LAB_PRICE_TRACE").is_ok();
    let mut iterations = 0u32;
    for _ in 0..max_iterations {
        iterations += 1;
        let mut bound = 0.0;
        for c in 0..n {
            bound += priced_viterbi(cross, terms, pairs, &current.lam[c], c, &mut weights[c]);
        }
        if trace {
            eprintln!("price iter {iterations}: bound={bound:.6} best={best_bound:.6} beta={beta:.4}");
        }
        if bound < best_bound - 1e-12 {
            best_bound = bound;
            best_lam.clone_from(&current.lam);
            stall = 0;
        } else {
            stall += 1;
            if stall >= 4 {
                // Shorten the step and restart from the best prices seen, so a
                // step that overshot does not become the base for the next.
                beta *= 0.5;
                stall = 0;
                current.lam.clone_from(&best_lam);
            }
        }
        if best_bound - lower <= 1e-7 || beta < 0.02 {
            break;
        }
        // Projected subgradient: each progeny's (spread) choice indicator minus
        // its mean over progeny, which keeps every (m, k) price column summing
        // to zero.
        let mut mean: Vec<Vec<f64>> = pairs.iter().map(|p| vec![0.0f64; p.len()]).collect();
        for c in 0..n {
            for m in 0..markers {
                for k in 0..pairs[m].len() {
                    mean[m][k] += weights[c][m][k] / n as f64;
                }
            }
        }
        let mut norm2 = 0.0f64;
        for c in 0..n {
            for m in 0..markers {
                for k in 0..pairs[m].len() {
                    let g = weights[c][m][k] - mean[m][k];
                    norm2 += g * g;
                }
            }
        }
        if norm2 <= 1e-12 {
            break; // every progeny agrees: the relaxation is exact here
        }
        let step = beta * (bound - lower).max(0.0) / norm2;
        if step <= 0.0 {
            break;
        }
        for c in 0..n {
            for m in 0..markers {
                for k in 0..pairs[m].len() {
                    current.lam[c][m][k] -= step * (weights[c][m][k] - mean[m][k]);
                }
            }
        }
    }
    Prices { lam: best_lam, bound: best_bound, iterations }
}
