//! Tightening the bound that the search prunes with.
//!
//! ## What the plain bound gives away
//!
//! A partially fixed parental assignment is scored by giving every progeny the
//! best score the settled markers can reach plus the best continuation the rest
//! could reach *if each progeny could take its own favourite column at every
//! later marker*. That last freedom is the giveaway: the real problem has one
//! column per marker shared by all progeny, and the bound hands each progeny a
//! private copy of the parents.
//!
//! ## Repairing it
//!
//! Give every progeny its own copy of the assignment, then charge for
//! disagreement. `spec/cross-model.md` aside, the arithmetic is the standard
//! splitting relaxation: with `lam[c][m][d]` the price progeny `c` pays for
//! taking column pair `d` at marker `m`,
//!
//! ```text
//! L(lam) = sum_c max over copies  ( score_c(copy) + sum_m lam[c][m][copy_m] )
//!          - sum_m min_d sum_c lam[c][m][d]
//! ```
//!
//! Any two copies that agree pay the same price at both ends, so the penalty
//! vanishes on the feasible set and `L` is above the optimum for every `lam` —
//! an upper bound, and `lam = 0` is exactly the plain bound. Minimising over
//! `lam` by subgradient ascent is what makes it tight.
//!
//! Two details make it usable inside the search. First, the per progeny inner
//! problem is still a Viterbi problem: the price enters the node score and the
//! column choice at a marker is independent of every other marker's, so the node
//! score is `dr + max_d (gl[dose] + lam[c][m][d])`. Second, adding a constant to
//! every `lam[c][m][.]` moves both terms by the same amount and leaves `L`
//! alone, so the multipliers are normalised to be non positive; that keeps every
//! node score non positive, which is what lets the search abandon a hopeless
//! node part way through summing progeny.

use crate::solver::branch::viterbi;
use crate::solver::model::{Model, NJ};

const NEG: f64 = f64::NEG_INFINITY;

/// Step length as a fraction of the Polyak step.
const STEP: f64 = 1.0;

/// Optimistic node scores, and the constant each depth owes.
pub struct Dual {
    /// `[progeny][marker][joint]` node score under the multipliers.
    nu: Vec<Vec<[f64; NJ]>>,
    /// `[marker]` the subtracted consensus term, summed over later markers.
    suffix: Vec<f64>,
}

impl Dual {
    /// Node scores of one progeny at one marker.
    pub fn nu(&self, progeny: usize, marker: usize) -> &[f64; NJ] {
        &self.nu[progeny][marker]
    }

    /// What a node at `marker` has subtracted from its bound.
    ///
    /// Empty beyond the last marker, so a leaf owes nothing.
    pub fn suffix(&self, marker: usize) -> f64 {
        self.suffix[marker]
    }

    /// The plain bound: every multiplier zero.
    pub fn trivial(model: &Model) -> Dual {
        let n = model.offspring;
        let m = model.markers;
        let mut nu = vec![vec![[NEG; NJ]; m]; n];
        for c in 0..n {
            for mm in 0..m {
                let mut row = [NEG; NJ];
                let g = &model.gl[c][mm];
                for j in 0..NJ {
                    if !model.legal[mm][j] {
                        continue;
                    }
                    let mut best = NEG;
                    let mask = model.dmask[mm][j];
                    for d in 0..5u8 {
                        if mask & (1 << d) != 0 && g[d as usize] > best {
                            best = g[d as usize];
                        }
                    }
                    row[j] = best + model.drj[mm][j];
                }
                nu[c][mm] = row;
            }
        }
        Dual { nu, suffix: vec![0.0; m] }
    }
}

/// What the multiplier search found.
pub struct DualOutcome {
    pub dual: Dual,
    /// The bound at the root: above the optimum, and below the plain bound.
    pub bound: f64,
    /// Assignments worth polishing: one per progeny's own copy, plus the
    /// consensus the multipliers settled on.
    pub candidates: Vec<Vec<(usize, usize)>>,
}

/// Minimises `L` over the multipliers, up to `iters` subgradient steps.
///
/// The returned bound is never worse than the plain one: `lam = 0` is the first
/// point visited, and the best point seen is the one kept.
pub fn optimise(model: &Model, lower_bound: f64, iters: usize) -> DualOutcome {
    let n = model.offspring;
    let m = model.markers;
    let nd: Vec<usize> = (0..m).map(|mm| model.na[mm] * model.nb[mm]).collect();
    let stride = nd.iter().cloned().max().unwrap_or(1).max(1);

    let mut lam = vec![0.0f64; n * m * stride];
    let mut best_lam = lam.clone();
    let mut best_l = f64::INFINITY;

    let mut nu = vec![vec![[NEG; NJ]; m]; n];
    let mut arg = vec![vec![[0u8; NJ]; m]; n];
    let mut chosen: Vec<Vec<usize>> = vec![vec![0usize; m]; n];

    for _ in 0..iters {
        fill(model, &lam, stride, &nd, &mut nu, &mut arg);

        // Each progeny takes its own best copy under the current prices.
        let mut total = 0.0f64;
        for c in 0..n {
            let (value, path) = viterbi(model, &nu[c]);
            total += value;
            for mm in 0..m {
                chosen[c][mm] = arg[c][mm][path[mm]] as usize;
            }
        }

        // The price the copies would pay if they all agreed.
        let (cons, cons_val) = consensus(model, &lam, stride, &nd);
        let level: f64 = cons_val.iter().sum();
        let value = total - level;

        if value < best_l {
            best_l = value;
            best_lam.copy_from_slice(&lam);
        }

        // Disagreement is the direction to walk: each progeny that picked a
        // different column pair than the consensus contributes two to the
        // squared length, one from its own indicator and one from the consensus.
        let mut square = 0.0f64;
        for c in 0..n {
            for mm in 0..m {
                if nd[mm] > 1 && chosen[c][mm] != cons[mm] {
                    square += 2.0;
                }
            }
        }
        if square == 0.0 {
            break;
        }
        let step = STEP * (value - lower_bound).max(0.0) / square;
        if step <= 0.0 {
            break;
        }
        for c in 0..n {
            for mm in 0..m {
                if nd[mm] <= 1 || chosen[c][mm] == cons[mm] {
                    continue;
                }
                let base = (c * m + mm) * stride;
                lam[base + chosen[c][mm]] -= step;
                lam[base + cons[mm]] += step;
            }
        }
        normalise(&mut lam, stride, &nd, m);
    }

    // The node scores the search will use, under the best prices found.
    fill(model, &best_lam, stride, &nd, &mut nu, &mut arg);
    let (cons, cons_val) = consensus(model, &best_lam, stride, &nd);
    let mut suffix = vec![0.0f64; m + 1];
    for mm in (0..m).rev() {
        suffix[mm] = suffix[mm + 1] + cons_val[mm];
    }

    // One candidate per progeny: its own copy, read off its best path. Plus the
    // assignment the copies were pushed towards.
    let mut candidates = Vec::with_capacity(n + 1);
    for c in 0..n {
        let (_, path) = viterbi(model, &nu[c]);
        candidates.push(
            (0..m)
                .map(|mm| {
                    let d = arg[c][mm][path[mm]] as usize;
                    (d / model.nb[mm], d % model.nb[mm])
                })
                .collect(),
        );
    }
    candidates.push(
        (0..m)
            .map(|mm| (cons[mm] / model.nb[mm], cons[mm] % model.nb[mm]))
            .collect(),
    );

    DualOutcome { dual: Dual { nu, suffix }, bound: best_l, candidates }
}

/// Node scores and the priced column pair that attains each of them.
fn fill(
    model: &Model,
    lam: &[f64],
    stride: usize,
    nd: &[usize],
    nu: &mut Vec<Vec<[f64; NJ]>>,
    arg: &mut Vec<Vec<[u8; NJ]>>,
) {
    let n = model.offspring;
    let m = model.markers;
    for c in 0..n {
        for mm in 0..m {
            let base = (c * m + mm) * stride;
            let g = &model.gl[c][mm];
            let dr = &model.drj[mm];
            let legal = &model.legal[mm];
            let tables = &model.dose[mm];
            let mut row = [NEG; NJ];
            let mut who = [0u8; NJ];
            for d in 0..nd[mm] {
                let price = lam[base + d];
                let dose = &tables[d];
                for j in 0..NJ {
                    if !legal[j] {
                        continue;
                    }
                    let v = g[dose[j] as usize] + price;
                    if v > row[j] {
                        row[j] = v;
                        who[j] = d as u8;
                    }
                }
            }
            for j in 0..NJ {
                if legal[j] {
                    row[j] += dr[j];
                }
            }
            nu[c][mm] = row;
            arg[c][mm] = who;
        }
    }
}

/// Per marker, the column pair the progeny would agree on and its total price.
fn consensus(
    model: &Model,
    lam: &[f64],
    stride: usize,
    nd: &[usize],
) -> (Vec<usize>, Vec<f64>) {
    let n = model.offspring;
    let m = model.markers;
    let mut cons = vec![0usize; m];
    let mut cons_val = vec![0.0f64; m];
    for mm in 0..m {
        let mut bd = 0usize;
        let mut bv = f64::INFINITY;
        for d in 0..nd[mm] {
            let mut sum = 0.0f64;
            for c in 0..n {
                sum += lam[(c * m + mm) * stride + d];
            }
            if sum < bv {
                bv = sum;
                bd = d;
            }
        }
        cons[mm] = bd;
        cons_val[mm] = bv;
    }
    (cons, cons_val)
}

/// Shifts each progeny's prices at each marker so the largest is zero.
///
/// `L` is unchanged, and every node score stays non positive, which is what the
/// search's early abandonment relies on.
fn normalise(lam: &mut [f64], stride: usize, nd: &[usize], m: usize) {
    let n = lam.len() / (m * stride);
    for c in 0..n {
        for mm in 0..m {
            let base = (c * m + mm) * stride;
            let mut top = NEG;
            for d in 0..nd[mm] {
                if lam[base + d] > top {
                    top = lam[base + d];
                }
            }
            if top != 0.0 {
                for d in 0..nd[mm] {
                    lam[base + d] -= top;
                }
            }
        }
    }
}
