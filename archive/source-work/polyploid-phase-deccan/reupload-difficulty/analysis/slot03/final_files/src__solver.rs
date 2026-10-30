//! The search.
//!
//! `solve` returns the reconstruction that maximises the total log likelihood
//! over every parental haplotype assignment consistent with `parent_dosage` and
//! every legal inheritance path, reduced to canonical form.
//!
//! The search has three stages.
//!
//! 1. A local search over allele columns gives a strong lower bound.
//! 2. Prices on the columns turn the coupled problem into one Viterbi per
//!    progeny and yield an upper bound; a subgradient pass drives it down.
//! 3. A branch and bound over markers closes the gap. It first maximises, then
//!    walks the same tree in canonical key order to pick the smallest
//!    haplotype pair among the assignments that tie at the optimum.
//!
//! Stages one and two only affect how fast stage three runs. Stage three
//! explores every assignment whose bound can still reach the optimum, so the
//! answer is the global one whatever the prices happen to be.

use crate::cross_input::Cross;
use crate::cross_model::{Parent, Reconstruction, HOMOLOGS};

use solver::bound::{optimise_prices, Backward};
use solver::canon::{canonicalise, distinct_key, remap_table, row_permutations};
use solver::model::{Model, NJ};
use solver::search::Searcher;
use solver::viterbi::{forward_backward, haplotypes_from_cols, lexicographic_path, propagate, total_all};

pub mod bound;
pub mod canon;
pub mod model;
pub mod search;
pub mod viterbi;

/// Finds the highest scoring reconstruction for a cross.
pub fn solve(cross: &Cross) -> Reconstruction {
    let model = Model::build(cross);
    let (lower, start) = local_search(&model, 24);
    let (prices, upper) = optimise_prices(&model, lower, 250);

    if solver::debug() {
        eprintln!(
            "markers={} offspring={} lower={:.6} dual={:.6}",
            model.m, model.n, lower, upper
        );
    }

    if std::env::var("PHASE_PRIMAL_ONLY").map(|v| v == "1").unwrap_or(false) {
        if crate::solver::debug() {
            eprintln!("primal only, skipping the search");
        }
        return assemble(&model, &start);
    }

    let back = Backward::build(&model, &prices);
    let mut searcher = Searcher::new(&model, &back);
    searcher.seed(lower, start.clone());
    let total = searcher.maximise();

    let columns = match searcher.collect(total - 1e-8) {
        Some(cols) => cols,
        None => searcher.best_cols.clone(),
    };

    if solver::debug() {
        eprintln!(
            "optimum={:.6} nodes={} leaves={}",
            total, searcher.nodes, searcher.leaves
        );
    }

    assemble(&model, &columns)
}

/// Builds the canonical reconstruction for a column sequence.
///
/// The paths are chosen so that the signature the result format orders on is as
/// small as it can be among the paths that attain the optimum. When two homologs
/// carry identical alleles every marker, which relabelling is canonical depends
/// on the paths, so every row sorting permutation is tried and the smallest
/// result wins.
fn assemble(model: &Model, cols: &[(usize, usize)]) -> Reconstruction {
    let hap_a = haplotypes_from_cols(model, Parent::A, cols);
    let hap_b = haplotypes_from_cols(model, Parent::B, cols);

    let mut best: Option<(Vec<u8>, Reconstruction)> = None;
    for pa in row_permutations(&hap_a) {
        for pb in row_permutations(&hap_b) {
            let remap_a = remap_table(&pa);
            let remap_b = remap_table(&pb);
            let mut paths = Vec::with_capacity(model.n);
            for c in 0..model.n {
                paths.push(lexicographic_path(model, c, cols, &remap_a, &remap_b));
            }
            let candidate = Reconstruction {
                haplotypes_a: hap_a.clone(),
                haplotypes_b: hap_b.clone(),
                paths,
            };
            let canonical = canonicalise(&candidate);
            let key = distinct_key(&canonical);
            if best.as_ref().map_or(true, |(k, _)| key < *k) {
                best = Some((key, canonical));
            }
        }
    }
    best.expect("there is always a candidate").1
}

// -------------------------------------------------------------------------
// Local search
// -------------------------------------------------------------------------

/// A deterministic linear congruential generator, so restarts are reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 17
    }
}

/// Multi start coordinate ascent over the allele columns.
fn local_search(model: &Model, restarts: usize, extra: &[Vec<(usize, usize)>]) -> (f64, Vec<(usize, usize)>) {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut best_value = f64::NEG_INFINITY;
    let mut best_cols = vec![(0usize, 0usize); model.m];

    for attempt in 0..restarts + extra.len() {
        let mut cols = if attempt < extra.len() {
            extra[attempt].clone()
        } else {
            match attempt - extra.len() {
                0 => vec![(0usize, 0usize); model.m],
                1 => (0..model.m)
                    .map(|mm| (model.cols[0][mm].len() - 1, model.cols[1][mm].len() - 1))
                    .collect(),
                _ => (0..model.m)
                    .map(|mm| {
                        let a = model.cols[0][mm].len();
                        let b = model.cols[1][mm].len();
                        ((rng.next() as usize) % a, (rng.next() as usize) % b)
                    })
                    .collect(),
            }
        };
        let value = polish(model, &mut cols);
        if value > best_value {
            best_value = value;
            best_cols = cols;
        }
    }
    (best_value, best_cols)
}

/// Repeated sweeps of coordinate ascent and refitting until nothing improves.
fn polish(model: &Model, cols: &mut Vec<(usize, usize)>) -> f64 {
    let mut value = total_all(model, cols);
    for _ in 0..64 {
        if sweep(model, cols) {
            let next = total_all(model, cols);
            if next <= value + 1e-12 {
                value = next;
                break;
            }
            value = next;
            continue;
        }
        if !refit(model, cols) {
            break;
        }
        let next = total_all(model, cols);
        if next <= value + 1e-12 {
            value = next;
            break;
        }
        value = next;
    }
    value
}

/// Reassigns every column to the best fit for the paths the columns induce.
///
/// With the paths held still the double reduction and transition terms are
/// fixed, so the column at each marker can be chosen outright, and the paths
/// that follow are no worse. Alternating the two ascends until neither moves.
fn refit(model: &Model, cols: &mut Vec<(usize, usize)>) -> bool {
    let paths: Vec<Vec<usize>> = (0..model.n).map(|c| best_path(model, c, cols)).collect();
    let mut changed = false;
    for mm in 0..model.m {
        let width = model.cols[1][mm].len();
        let mut best_pair = 0usize;
        let mut best_value = f64::NEG_INFINITY;
        for p in 0..model.pairs[mm].len() {
            let row = &model.dose[mm][p];
            let mut total = 0.0f64;
            for c in 0..model.n {
                total += model.gl[c][mm][usize::from(row[paths[c][mm]])];
            }
            if total > best_value {
                best_value = total;
                best_pair = p;
            }
        }
        let next = (best_pair / width, best_pair % width);
        if next != cols[mm] {
            cols[mm] = next;
            changed = true;
        }
    }
    changed
}

/// The columns most of the progeny settle on when each may choose its own.
fn consensus(model: &Model, prices: &Prices) -> Vec<(usize, usize)> {
    let (_, picks) = dual_value(model, prices, true);
    let mut out = Vec::with_capacity(model.m);
    for mm in 0..model.m {
        let width = model.pairs[mm].len();
        let mut count = vec![0u32; width];
        for c in 0..model.n {
            count[picks[c][mm]] += 1;
        }
        let mut best = 0usize;
        for p in 1..width {
            if count[p] > count[best] {
                best = p;
            }
        }
        let (a, b) = model.pairs[mm][best];
        out.push((a, b));
    }
    out
}

/// One pass over the markers, replacing each column pair by the best one given
/// the rest.
fn sweep(model: &Model, cols: &mut Vec<(usize, usize)>) -> bool {
    let m = model.m;
    let n = model.n;
    let mut fwd = Vec::with_capacity(n);
    let mut bwd = Vec::with_capacity(n);
    for c in 0..n {
        let (f, b, _) = forward_backward(model, c, cols);
        fwd.push(f);
        bwd.push(b);
    }

    let mut up = vec![[0.0f64; NJ]; n];
    let mut down = vec![[0.0f64; NJ]; n];
    let mut changed = false;

    for mm in 0..m {
        for c in 0..n {
            if mm == 0 {
                up[c] = [0.0f64; NJ];
            } else {
                let mut u = [f64::NEG_INFINITY; NJ];
                propagate(&fwd[c][mm - 1], &model.trans[0][mm - 1], &model.trans[1][mm - 1], &mut u);
                up[c] = u;
            }
            if mm + 1 == m {
                down[c] = [0.0f64; NJ];
            } else {
                let mut w = [f64::NEG_INFINITY; NJ];
                propagate(&bwd[c][mm + 1], &model.trans_t[0][mm], &model.trans_t[1][mm], &mut w);
                down[c] = w;
            }
        }

        let mut best_pair = None;
        let mut best_value = f64::NEG_INFINITY;
        for pair in 0..model.pairs[mm].len() {
            let row = &model.dose[mm][pair];
            let mut total = 0.0f64;
            for c in 0..n {
                let gl = &model.gl[c][mm];
                let drj = &model.drj[mm];
                let up_c = &up[c];
                let down_c = &down[c];
                let mut best = f64::NEG_INFINITY;
                for s in 0..NJ {
                    let v = up_c[s] + gl[usize::from(row[s])] + drj[s] + down_c[s];
                    if v > best {
                        best = v;
                    }
                }
                total += best;
            }
            if total > best_value {
                best_value = total;
                best_pair = Some(pair);
            }
        }

        if let Some(pair) = best_pair {
            let width = model.cols[1][mm].len();
            let next = (pair / width, pair % width);
            if next != cols[mm] {
                cols[mm] = next;
                changed = true;
            }
        }
    }
    changed
}

/// Whether the search should report progress on standard error.
pub fn debug() -> bool {
    std::env::var("PHASE_DEBUG").map(|v| v == "1").unwrap_or(false)
}

/// Re-exported for the diagnostics.
pub const HOMOLOG_COUNT: usize = HOMOLOGS;
