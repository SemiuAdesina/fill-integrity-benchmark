//! Branch and bound over the parental allele columns.
//!
//! Markers are assigned in order, both parents at each marker, and the progeny
//! forward values are extended one marker at a time as the branch deepens. The
//! bound at a partly assigned node is the exact value of the assigned prefix
//! plus a priced, per progeny optimistic continuation of the rest.
//!
//! Branches are restricted to haplotype matrices whose four rows are in
//! ascending order. Relabelling the homologs of a parent permutes its rows and
//! leaves the score alone, so every reconstruction has exactly one such
//! representative and discarding the others costs nothing.

use super::bound::Backward;
use super::model::{key_bits, Model, PAIRS};
use super::viterbi::propagate;
use super::model::{NJ, NS};

/// A row ordering decision: bit `k` says rows `i < j` already differ somewhere
/// with row `i` below row `j`, so later markers may say anything.
fn step_decided(dec: u16, mask: u8) -> Option<u16> {
    let mut out = dec;
    for (k, &(i, j)) in PAIRS.iter().enumerate() {
        let bi = (mask >> i) & 1;
        let bj = (mask >> j) & 1;
        if bi == bj {
            continue;
        }
        let bit = 1u16 << k;
        if out & bit != 0 {
            continue;
        }
        if bi > bj {
            return None;
        }
        out |= bit;
    }
    Some(out)
}

/// For every row ordering state and marker, the admissible column whose four
/// bits are smallest from the top row down.
fn bound_columns(model: &Model) -> [Vec<Vec<u8>>; 2] {
    let m = model.m;
    let mut out = [Vec::new(), Vec::new()];
    for p in 0..2 {
        let mut per_dec = Vec::with_capacity(64);
        for dec in 0..64u16 {
            let mut per_marker = Vec::with_capacity(m);
            for mm in 0..m {
                let mut best = 0u8;
                let mut best_key = u8::MAX;
                for &mask in &model.cols[p][mm] {
                    if step_decided(dec, mask).is_none() {
                        continue;
                    }
                    let k = key_bits(mask);
                    if k < best_key {
                        best_key = k;
                        best = mask;
                    }
                }
                per_marker.push(best);
            }
            per_dec.push(per_marker);
        }
        out[p] = per_dec;
    }
    out
}

/// `[marker]` the most the shared columns can earn at that marker with every
/// progeny in its best state, and the suffix sums of those values.
fn marker_bounds(model: &Model) -> (Vec<f64>, Vec<f64>) {
    let m = model.m;
    let mut best = vec![f64::NEG_INFINITY; m];
    for mm in 0..m {
        let mut top = f64::NEG_INFINITY;
        for p in 0..model.pairs[mm].len() {
            let row = &model.dose[mm][p];
            let mut total = 0.0f64;
            for c in 0..model.n {
                let gl = &model.gl[c][mm];
                let mut best_state = f64::NEG_INFINITY;
                for s in 0..NJ {
                    let v = gl[usize::from(row[s])] + model.drj[mm][s];
                    if v > best_state {
                        best_state = v;
                    }
                }
                total += best_state;
            }
            if total > top {
                top = total;
            }
        }
        best[mm] = top;
    }
    let mut suffix = vec![0.0f64; m + 1];
    for mm in (0..m).rev() {
        suffix[mm] = suffix[mm + 1] + best[mm];
    }
    (best, suffix)
}

/// `[marker]` what the progeny could save in total by never recombining again,
/// from the interval starting at that marker on.
fn jump_suffix(model: &Model) -> Vec<f64> {
    let m = model.m;
    let mut suffix = vec![0.0f64; m + 1];
    for mm in (0..m.saturating_sub(1)).rev() {
        let mut top = 0.0f64;
        for p in 0..2 {
            let mut best = f64::NEG_INFINITY;
            for a in 0..NS {
                for b in 0..NS {
                    if model.trans[p][mm][a][b] > best {
                        best = model.trans[p][mm][a][b];
                    }
                }
            }
            top += best;
        }
        suffix[mm] = suffix[mm + 1] + top * model.n as f64;
    }
    suffix
}

pub struct Searcher<'a> {
    model: &'a Model,
    back: &'a Backward,
    /// `[progeny][marker]` forward values at that marker.
    fwd: Vec<Vec<[f64; NJ]>>,
    /// Scratch for the values carried into the marker being branched on.
    ubuf: Vec<[f64; NJ]>,
    /// `[progeny][marker]` the carried in values plus the double reduction term.
    udr: Vec<Vec<[f64; NJ]>>,
    /// `[progeny][marker]` the above plus the priced continuation.
    base: Vec<Vec<[f64; NJ]>>,
    cols: Vec<(usize, usize)>,
    /// `[parent][decided][marker]` the smallest column the row ordering still
    /// admits, used to bound an open marker's contribution to the key.
    bound_cols: [Vec<Vec<u8>>; 2],
    /// `[marker]` the most the shared columns could still earn from that
    /// marker on, with every progeny free to sit in any state.
    marker_best: Vec<f64>,
    /// `[marker]` suffix sums of `marker_best`.
    marker_suffix: Vec<f64>,
    /// `[marker]` suffix sums of the best transition an interval can give,
    /// times the number of progeny, over the intervals from that marker on.
    jump_suffix: Vec<f64>,
    pub best_val: f64,
    pub best_cols: Vec<(usize, usize)>,
    cutoff: f64,
    best_key: Option<Vec<u8>>,
    collect_cols: Option<Vec<(usize, usize)>>,
    pub nodes: u64,
    pub leaves: u64,
}

impl<'a> Searcher<'a> {
    pub fn new(model: &'a Model, back: &'a Backward) -> Searcher<'a> {
        let n = model.n;
        let m = model.m;
        Searcher {
            model,
            back,
            fwd: vec![vec![[f64::NEG_INFINITY; NJ]; m]; n],
            ubuf: vec![[0.0; NJ]; n],
            udr: vec![vec![[0.0; NJ]; m]; n],
            base: vec![vec![[0.0; NJ]; m]; n],
            cols: vec![(0, 0); m],
            bound_cols: bound_columns(model),
            marker_best: marker_bounds(model).0,
            marker_suffix: marker_bounds(model).1,
            jump_suffix: jump_suffix(model),
            best_val: f64::NEG_INFINITY,
            best_cols: vec![(0, 0); m],
            cutoff: f64::NEG_INFINITY,
            best_key: None,
            collect_cols: None,
            nodes: 0,
            leaves: 0,
        }
    }

    /// Starts the search from a known assignment and its value.
    pub fn seed(&mut self, value: f64, cols: Vec<(usize, usize)>) {
        self.best_val = value;
        self.best_cols = cols;
    }

    fn enter(&mut self, d: usize) {
        let n = self.model.n;
        if d == 0 {
            for c in 0..n {
                self.ubuf[c] = [0.0f64; NJ];
            }
        } else {
            for c in 0..n {
                let mut u = [f64::NEG_INFINITY; NJ];
                propagate(
                    &self.fwd[c][d - 1],
                    &self.model.trans[0][d - 1],
                    &self.model.trans[1][d - 1],
                    &mut u,
                );
                self.ubuf[c] = u;
            }
        }
        let drj = &self.model.drj[d];
        for c in 0..n {
            let carried = self.ubuf[c];
            let udr = &mut self.udr[c][d];
            for s in 0..NJ {
                udr[s] = carried[s] + drj[s];
            }
        }
        for c in 0..n {
            let pull = &self.back.pull[c][d];
            let udr = &self.udr[c][d];
            let base = &mut self.base[c][d];
            for s in 0..NJ {
                base[s] = udr[s] + pull[s];
            }
        }
    }

    /// The better of the two continuations of a child: the priced one, and the
    /// one that keeps the columns shared but lets every progeny jump between
    /// states paying no more than the best transition an interval can give.
    fn child_bound(&self, d: usize, pair: usize) -> f64 {
        let row = &self.model.dose[d][pair];
        let n = self.model.n;
        let mut priced = 0.0f64;
        let mut shared = self.marker_suffix[d + 1] + self.jump_suffix[d];
        for c in 0..n {
            let gl = &self.model.gl[c][d];
            let base = &self.base[c][d];
            let udr = &self.udr[c][d];
            let mut best_priced = f64::NEG_INFINITY;
            let mut best_shared = f64::NEG_INFINITY;
            for s in 0..NJ {
                let g = gl[usize::from(row[s])];
                let v = base[s] + g;
                if v > best_priced {
                    best_priced = v;
                }
                let w = udr[s] + g;
                if w > best_shared {
                    best_shared = w;
                }
            }
            priced += best_priced;
            shared += best_shared;
        }
        if priced < shared {
            priced
        } else {
            shared
        }
    }

    fn push_forward(&mut self, d: usize, pair: usize) {
        let row = &self.model.dose[d][pair];
        for c in 0..self.model.n {
            let gl = &self.model.gl[c][d];
            let udr = self.udr[c][d];
            let target = &mut self.fwd[c][d];
            for s in 0..NJ {
                target[s] = udr[s] + gl[usize::from(row[s])];
            }
        }
    }

    fn key_of(&self) -> Vec<u8> {
        build_key(self.model, &self.cols)
    }

    /// Whether every completion of the markers assigned so far has a row major
    /// key that sorts after `best`.
    ///
    /// The bound is the key of the first `known` markers followed, at every
    /// marker still open, by the smallest column the row ordering still admits.
    /// Each open marker contributes all four of its bits from that one column,
    /// so the bound respects the fact that a row can only be lowered at a
    /// marker by raising another: row by row it is never above what a
    /// completion can reach, and it therefore never sorts after one. Pruning on
    /// it keeps the smallest key among the assignments that tie without walking
    /// the ones that can only sort later.
    fn sorts_after(&self, known: usize, dec_a: u16, dec_b: u16, best: &[u8]) -> bool {
        let m = self.model.m;
        let dec = [dec_a, dec_b];
        let mut k = 0usize;
        for p in 0..2 {
            let bound = &self.bound_cols[p][usize::from(dec[p])];
            let columns = &self.model.cols[p];
            for h in 0..4u8 {
                for mm in 0..m {
                    let bit = if mm < known {
                        let i = if p == 0 { self.cols[mm].0 } else { self.cols[mm].1 };
                        (columns[mm][i] >> h) & 1
                    } else {
                        (bound[mm] >> h) & 1
                    };
                    if bit > best[k] {
                        return true;
                    }
                    if bit < best[k] {
                        return false;
                    }
                    k += 1;
                }
            }
        }
        false
    }

    /// Finds the maximum total, returning it with the column sequence that
    /// attains it.
    pub fn maximise(&mut self) -> f64 {
        self.dfs(0, 0, 0, false);
        self.best_val
    }

    /// Finds the smallest canonical haplotype pair among every assignment whose
    /// total is at least `cutoff`.
    pub fn collect(&mut self, cutoff: f64) -> Option<Vec<(usize, usize)>> {
        self.cutoff = cutoff;
        self.best_key = None;
        self.collect_cols = None;
        self.dfs(0, 0, 0, true);
        self.collect_cols.clone()
    }

    fn dfs(&mut self, d: usize, dec_a: u16, dec_b: u16, collect: bool) {
        if collect {
            if let Some(key) = &self.best_key {
                if self.sorts_after(d, dec_a, dec_b, key) {
                    return;
                }
            }
        }
        self.nodes += 1;
        if crate::solver::trace() {
            eprintln!(
                "depth {d} dec_a {dec_a:06b} dec_b {dec_b:06b} best_key {:?}",
                self.best_key.as_ref().map(|k| k.len())
            );
        }
        self.enter(d);
        let m = self.model.m;
        let width_a = self.model.cols[0][d].len();
        let width_b = self.model.cols[1][d].len();

        let mut kids: Vec<(f64, usize, u16, u16)> = Vec::with_capacity(width_a * width_b);
        for ia in 0..width_a {
            let mask_a = self.model.cols[0][d][ia];
            let nda = match step_decided(dec_a, mask_a) {
                Some(x) => x,
                None => continue,
            };
            for ib in 0..width_b {
                let mask_b = self.model.cols[1][d][ib];
                let ndb = match step_decided(dec_b, mask_b) {
                    Some(x) => x,
                    None => continue,
                };
                let pair = ia * width_b + ib;
                let bound = self.child_bound(d, pair);
                kids.push((bound, pair, nda, ndb));
            }
        }

        if crate::solver::trace() {
            let floor = if collect {
                self.cutoff - 1e-9
            } else {
                self.best_val + 1e-9
            };
            eprintln!(
                "  node d={d} kids={} bounds={:?} floor={floor:.8}",
                kids.len(),
                kids.iter().map(|(b, _, _, _)| format!("{b:.8}")).collect::<Vec<_>>()
            );
        }
        if collect {
            // Key order: the columns were generated in canonical key order.
            let floor = self.cutoff - 1e-9;
            kids.retain(|(b, _, _, _)| *b >= floor);
        } else {
            kids.retain(|(b, _, _, _)| *b > self.best_val + 1e-9);
            kids.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        }

        for (bound, pair, nda, ndb) in kids {
            self.cols[d] = (pair / width_b, pair % width_b);
            if collect {
                if let Some(key) = &self.best_key {
                    if self.sorts_after(d + 1, nda, ndb, key) {
                        continue;
                    }
                }
            }
            self.push_forward(d, pair);
            if d + 1 == m {
                self.leaves += 1;
                let value = bound;
                if collect {
                    if value >= self.cutoff - 1e-9 {
                        let key = self.key_of();
                        if self.best_key.as_ref().map_or(true, |k| key < *k) {
                            self.best_key = Some(key);
                            self.collect_cols = Some(self.cols.clone());
                        }
                    }
                } else if value > self.best_val {
                    self.best_val = value;
                    self.best_cols = self.cols.clone();
                }
            } else {
                self.dfs(d + 1, nda, ndb, collect);
            }
        }
    }
}

/// Flat row major key of a haplotype pair, parent A first.
pub fn build_key(model: &Model, cols: &[(usize, usize)]) -> Vec<u8> {
    let m = model.m;
    let mut out = Vec::with_capacity(8 * m);
    for p in 0..2 {
        for h in 0..4 {
            for mm in 0..m {
                out.push((model.cols[p][mm][if p == 0 { cols[mm].0 } else { cols[mm].1 }] >> h) & 1);
            }
        }
    }
    out
}
