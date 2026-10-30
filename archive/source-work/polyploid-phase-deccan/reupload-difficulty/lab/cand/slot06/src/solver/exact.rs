
//! An exact search over the marker chain, for the crosses small enough to close.
//!
//! Every progeny's path is a chain, so the only thing left to decide is the
//! column of each marker: which homologs of parent A and which of parent B carry
//! an allele there. Branch on those, left to right, and the part of the chain
//! already fixed can be scored exactly — one forward pass per progeny, all of
//! them in lockstep — while the rest is bounded by [`Relaxed`].
//!
//! The tree is searched depth first, each node's children ordered by their bound
//! so that a good leaf is reached early and the rest of the tree is pruned
//! against it. It is the order that makes this affordable: the same bound that
//! is far too loose to guide a greedy choice is tight enough, a few markers from
//! the end, to close off nearly everything.
//!
//! Branching on raw columns would visit every relabelling of the same answer
//! four factorial times over per parent. Instead the rows are built up under the
//! requirement that they never come out the wrong way round, so exactly one
//! relabelling of each solution is ever reached.

use std::collections::HashSet;

use crate::solver::relaxed::{advance, Relaxed};
use crate::solver::search::record;
use crate::solver::tables::{Ctx, NJ, NS};

/// Slack under which two totals are taken to be the same. Well below the `1e-8`
/// the specification allows, so it cannot hide a real gain.
const TOL: f64 = 1e-9;

/// Branch and bound over the column of every marker.
pub struct Exact<'a> {
    ctx: &'a Ctx,
    relaxed: Relaxed,
    best: f64,
    ca: Vec<u8>,
    cb: Vec<u8>,
    best_ca: Vec<u8>,
    best_cb: Vec<u8>,
    nodes: u64,
    limit: u64,
    /// For each parent and each pair of rows, whether the pair is still tied
    /// (`0`) or the first has already come out above the second (`1`). The
    /// opposite order is never reached, which keeps one representative of each
    /// relabelling.
    order: [[u8; 6]; 2],
    ties: Vec<(Vec<u8>, Vec<u8>)>,
    seen: HashSet<(Vec<u8>, Vec<u8>)>,
}

impl<'a> Exact<'a> {
    /// Prepares the bounds. An incumbent may be supplied with [`Exact::seed`]
    /// before running; without one every node is visited, which is slower but
    /// still correct.
    pub fn new(ctx: &'a Ctx, limit: u64) -> Exact<'a> {
        Exact {
            ctx,
            relaxed: Relaxed::new(ctx),
            best: f64::NEG_INFINITY,
            ca: vec![0u8; ctx.m],
            cb: vec![0u8; ctx.m],
            best_ca: vec![0u8; ctx.m],
            best_cb: vec![0u8; ctx.m],
            nodes: 0,
            limit,
            order: [[0u8; 6]; 2],
            ties: Vec::new(),
            seen: HashSet::new(),
        }
    }

    /// Supplies a total already attained, so the search can prune against it.
    pub fn seed(&mut self, value: f64) {
        if value > self.best {
            self.best = value;
        }
    }

    /// Whether a total can be shown to be the optimum without searching at all,
    /// because the bound on the whole chain agrees with it.
    pub fn proves(&self, value: f64) -> bool {
        self.relaxed.root_bound() <= value + TOL
    }

    /// The best total found, which is the optimum when the search completed.
    pub fn best(&self) -> f64 {
        self.best
    }

    /// A column assignment attaining [`Exact::best`].
    pub fn columns(&self) -> (Vec<u8>, Vec<u8>) {
        (self.best_ca.clone(), self.best_cb.clone())
    }

    /// Every column assignment found that attains [`Exact::best`].
    pub fn ties(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.ties.clone()
    }

    /// How many assignments the search has examined.
    pub fn nodes(&self) -> u64 {
        self.nodes
    }

    /// Searches. Returns whether the whole tree was closed, in which case
    /// [`Exact::best`] is the optimum; otherwise the node limit stopped it and
    /// what it reached is still an improvement on the incumbent, if any.
    pub fn run(&mut self) -> bool {
        if self.ctx.m == 0 {
            return true;
        }
        let start: Vec<[f64; NJ]> = (0..self.ctx.n).map(|_| [f64::NEG_INFINITY; NJ]).collect();
        self.dfs(0, &start, 0.0)
    }

    /// Extends one parent's partial rows by a column, rejecting the branch when
    /// two rows have already come out the wrong way round.
    pub fn extend(&self, parent: usize, mask: u8) -> Option<[u8; 6]> {
        let bits = [
            (mask >> 0) & 1,
            (mask >> 1) & 1,
            (mask >> 2) & 1,
            (mask >> 3) & 1,
        ];
        let mut out = self.order[parent];
        let mut slot = 0usize;
        for i in 0..4 {
            for j in i + 1..4 {
                if out[slot] == 0 {
                    if bits[i] > bits[j] {
                        return None;
                    }
                    if bits[i] < bits[j] {
                        out[slot] = 1;
                    }
                }
                slot += 1;
            }
        }
        Some(out)
    }

    fn dfs(&mut self, k: usize, fwd: &[[f64; NJ]], shared: f64) -> bool {
        if k == self.ctx.m {
            let mut total = 0.0f64;
            for row in fwd.iter() {
                let mut best = f64::NEG_INFINITY;
                for v in row.iter() {
                    if *v > best {
                        best = *v;
                    }
                }
                total += best;
            }
            record(
                &mut self.best,
                &mut self.ties,
                &mut self.seen,
                total,
                &self.ca,
                &self.cb,
            );
            if total >= self.best - TOL {
                self.best_ca.copy_from_slice(&self.ca);
                self.best_cb.copy_from_slice(&self.cb);
            }
            return true;
        }

        let ctx = self.ctx;
        let na = ctx.ncol[0][k];
        let nb = ctx.ncol[1][k];
        let mut children: Vec<(usize, usize, f64, f64, Vec<[f64; NJ]>, [u8; 6], [u8; 6])> =
            Vec::with_capacity(na * nb);
        for ia in 0..na {
            let order_a = match self.extend(0, ctx.cols[0][k][ia]) {
                Some(order) => order,
                None => continue,
            };
            for ib in 0..nb {
                let order_b = match self.extend(1, ctx.cols[1][k][ib]) {
                    Some(order) => order,
                    None => continue,
                };
                let row = &ctx.dose[k][ia * nb + ib];
                let mut next: Vec<[f64; NJ]> = Vec::with_capacity(ctx.n);
                let mut paths = 0.0f64;
                let mut free_part = 0.0f64;
                let mut feasible = true;
                for c in 0..ctx.n {
                    let mut cur = [f64::NEG_INFINITY; NJ];
                    advance(ctx, k, c, row, &fwd[c], &mut cur);
                    let tail = self.relaxed.suffix[k + 1][c];
                    let mut best = f64::NEG_INFINITY;
                    for u in 0..NJ {
                        let v = cur[u] + tail[u];
                        if v > best {
                            best = v;
                        }
                    }
                    if best == f64::NEG_INFINITY {
                        feasible = false;
                        break;
                    }
                    paths += best;
                    let mut best_free = f64::NEG_INFINITY;
                    for j in 0..NJ {
                        let v = ctx.gl[c][k][row[j] as usize]
                            + ctx.drv[0][k][j / NS]
                            + ctx.drv[1][k][j % NS];
                        if v > best_free {
                            best_free = v;
                        }
                    }
                    free_part += best_free;
                    next.push(cur);
                }
                if !feasible {
                    continue;
                }
                let bound = self.relaxed.bound(k, paths, shared + free_part);
                children.push((ia, ib, bound, free_part, next, order_a, order_b));
            }
        }
        // Most promising first: the sooner a good leaf is reached, the more of
        // what is left is pruned away.
        children.sort_by(|x, y| y.2.partial_cmp(&x.2).unwrap_or(std::cmp::Ordering::Equal));

        for (ia, ib, bound, free_part, next, order_a, order_b) in children {
            self.nodes += 1;
            if self.nodes > self.limit {
                return false;
            }
            if bound <= self.best - TOL {
                continue;
            }
            let held_a = self.ca[k];
            let held_b = self.cb[k];
            let held_order = self.order;
            self.ca[k] = ia as u8;
            self.cb[k] = ib as u8;
            self.order = [order_a, order_b];
            let done = self.dfs(k + 1, &next, shared + free_part);
            self.ca[k] = held_a;
            self.cb[k] = held_b;
            self.order = held_order;
            if !done {
                return false;
            }
        }
        true
    }
}
