
//! The search over parental allele columns.
//!
//! Progeny are conditionally independent given the two parents' haplotypes, so
//! a column assignment is scored by one Viterbi pass per progeny. The search
//! alternates two exact conditional improvements and wraps them in restarts and
//! perturbations:
//!
//! * given the columns, every progeny's best path (Viterbi);
//! * given the paths, every marker's best column pair;
//! * and a stronger single marker move that re-optimises every progeny's path
//!   while the marker's columns change, read off the forward and backward values
//!   of the current Viterbi pass.
//!
//! Where the admissible column space is small enough to enumerate, it is
//! enumerated instead, with a per progeny upper bound pruning the inner loop.

use std::collections::HashSet;

use crate::solver::tables::{Ctx, Prog, NJ, NS};

/// Slack used when deciding whether a move improves on the incumbent. Well
/// below the `1e-8` the specification allows, so it cannot hide a real gain.
const TOL: f64 = 1e-9;

/// Deterministic xorshift generator, so repeated runs are byte identical.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

/// Every progeny's Viterbi pass against one column assignment.
pub struct Eval {
    pub total: f64,
    pub progs: Vec<Prog>,
}

/// Scores a column assignment, keeping the forward and backward values.
pub fn eval_all(ctx: &Ctx, ca: &[u8], cb: &[u8]) -> Eval {
    let mut progs = Vec::with_capacity(ctx.n);
    let mut total = 0.0f64;
    for c in 0..ctx.n {
        let prog = ctx.viterbi(c, ca, cb);
        total += prog.val;
        progs.push(prog);
    }
    Eval { total, progs }
}

/// Best column pair for every marker, holding the paths fixed.
fn reseat_columns(ctx: &Ctx, paths: &[Vec<usize>], ca: &mut [u8], cb: &mut [u8]) {
    for mm in 0..ctx.m {
        let na = ctx.ncol[0][mm];
        let nb = ctx.ncol[1][mm];
        if na * nb == 1 {
            continue;
        }
        let mut best = f64::NEG_INFINITY;
        let mut best_pair = (0usize, 0usize);
        for ia in 0..na {
            for ib in 0..nb {
                let d = &ctx.dose[mm][ia * nb + ib];
                let mut sum = 0.0f64;
                for (c, path) in paths.iter().enumerate() {
                    let j = path[mm];
                    sum += ctx.gl[c][mm][d[j] as usize];
                }
                if sum > best {
                    best = sum;
                    best_pair = (ia, ib);
                }
            }
        }
        ca[mm] = best_pair.0 as u8;
        cb[mm] = best_pair.1 as u8;
    }
}

/// For every marker, the column pair that most improves the total when every
/// progeny is allowed to re-route its path, and by how much.
///
/// The forward and backward values give the best total of any path that passes
/// through joint state `j` at marker `mm`, so changing that marker's columns
/// while everything else stays put moves the total by
///
/// ```text
/// max_j (best through j) - value,  with the emission swapped
/// ```
///
/// which is exact: only one marker's node potentials changed.
fn scan_moves(ctx: &Ctx, eval: &Eval, ca: &[u8], cb: &[u8]) -> Vec<Option<(usize, usize, f64)>> {
    let mut moves = Vec::with_capacity(ctx.m);
    for mm in 0..ctx.m {
        let na = ctx.ncol[0][mm];
        let nb = ctx.ncol[1][mm];
        if na * nb == 1 {
            moves.push(None);
            continue;
        }
        let current = &ctx.dose[mm][ca[mm] as usize * nb + cb[mm] as usize];
        let dr_a = ctx.drv[0][mm];
        let dr_b = ctx.drv[1][mm];
        let mut best_pair = (0usize, 0usize);
        let mut best_delta = f64::NEG_INFINITY;
        for ia in 0..na {
            for ib in 0..nb {
                let candidate = &ctx.dose[mm][ia * nb + ib];
                let mut delta = 0.0f64;
                for (c, prog) in eval.progs.iter().enumerate() {
                    let gl = ctx.gl[c][mm];
                    let fwd = prog.fwd[mm];
                    let bwd = prog.bwd[mm];
                    let mut value = f64::NEG_INFINITY;
                    for j in 0..NJ {
                        // Best total of any path through joint state `j`, with
                        // this marker's emission stripped out: the double
                        // reduction terms cancel against themselves, so only
                        // the emission has to be added and removed.
                        let through = fwd[j] + bwd[j]
                            - gl[current[j] as usize]
                            - dr_a[j / NS]
                            - dr_b[j % NS];
                        let swapped = through - gl[current[j] as usize] + gl[candidate[j] as usize];
                        if swapped > value {
                            value = swapped;
                        }
                    }
                    delta += value - prog.val;
                }
                if delta > best_delta {
                    best_delta = delta;
                    best_pair = (ia, ib);
                }
            }
        }
        moves.push(Some((best_pair.0, best_pair.1, best_delta)));
    }
    moves
}

/// Runs the two conditional improvements to a fixed point.
///
/// Returns the total, leaving `ca` and `cb` at the best assignment reached.
pub fn local_search(ctx: &Ctx, ca: &mut Vec<u8>, cb: &mut Vec<u8>) -> f64 {
    let mut eval = eval_all(ctx, ca, cb);
    for _ in 0..2 * ctx.m + 8 {
        let paths: Vec<Vec<usize>> = eval.progs.iter().map(|p| p.path()).collect();
        let held_a = ca.clone();
        let held_b = cb.clone();
        reseat_columns(ctx, &paths, ca, cb);
        let moved = eval_all(ctx, ca, cb);
        if moved.total > eval.total + TOL {
            eval = moved;
        } else {
            ca.copy_from_slice(&held_a);
            cb.copy_from_slice(&held_b);
        }

        let moves = scan_moves(ctx, &eval, ca, cb);
        let mut best_delta = TOL;
        let mut chosen: Option<(usize, usize, usize)> = None;
        let mut any = false;
        for (mm, mv) in moves.iter().enumerate() {
            if let Some((ia, ib, delta)) = mv {
                if *delta > TOL {
                    any = true;
                    if *delta > best_delta {
                        best_delta = *delta;
                        chosen = Some((mm, *ia, *ib));
                    }
                }
            }
        }
        if !any {
            break;
        }

        let held_a = ca.clone();
        let held_b = cb.clone();
        for (mm, mv) in moves.iter().enumerate() {
            if let Some((ia, ib, delta)) = mv {
                if *delta > TOL {
                    ca[mm] = *ia as u8;
                    cb[mm] = *ib as u8;
                }
            }
        }
        let moved = eval_all(ctx, ca, cb);
        if moved.total > eval.total + TOL {
            eval = moved;
            continue;
        }
        ca.copy_from_slice(&held_a);
        cb.copy_from_slice(&held_b);
        if let Some((mm, ia, ib)) = chosen {
            ca[mm] = ia as u8;
            cb[mm] = ib as u8;
            let moved = eval_all(ctx, ca, cb);
            if moved.total > eval.total + TOL {
                eval = moved;
                continue;
            }
            ca.copy_from_slice(&held_a);
            cb.copy_from_slice(&held_b);
        }
        break;
    }
    eval.total
}

/// What the search concluded: the best total and every column assignment seen
/// that attains it.
pub struct Outcome {
    pub value: f64,
    pub assignments: Vec<(Vec<u8>, Vec<u8>)>,
}

/// Upper bound on one progeny's contribution, over every column assignment and
/// every path. Used only to prune exhaustive enumeration.
fn progeny_bound(ctx: &Ctx, c: usize) -> f64 {
    let mut total = 0.0f64;
    for mm in 0..ctx.m {
        let mut best = f64::NEG_INFINITY;
        for row in ctx.dose[mm].iter() {
            for j in 0..NJ {
                let v = ctx.gl[c][mm][row[j] as usize]
                    + ctx.drv[0][mm][j / NS]
                    + ctx.drv[1][mm][j % NS];
                if v > best {
                    best = v;
                }
            }
        }
        total += best;
        if mm + 1 < ctx.m {
            for pi in 0..2 {
                let term = &ctx.tr[pi][mm];
                let mut best_t = f64::NEG_INFINITY;
                for v in term.iter() {
                    if *v > best_t {
                        best_t = *v;
                    }
                }
                total += best_t;
            }
        }
    }
    total
}

/// Enumerates every admissible column assignment. Only used where the space is
/// small; the bound prunes most of the work.
fn enumerate(ctx: &Ctx) -> Outcome {
    let mut radix = vec![0usize; ctx.m];
    let mut stride = vec![0usize; ctx.m];
    let mut space = 1u128;
    for mm in 0..ctx.m {
        stride[mm] = ctx.ncol[0][mm] * ctx.ncol[1][mm];
        space *= stride[mm] as u128;
    }
    let bounds: Vec<f64> = (0..ctx.n).map(|c| progeny_bound(ctx, c)).collect();
    let suffix: Vec<f64> = {
        let mut out = vec![0.0f64; ctx.n + 1];
        for c in (0..ctx.n).rev() {
            out[c] = out[c + 1] + bounds[c];
        }
        out
    };

    let mut best = f64::NEG_INFINITY;
    let mut assignments: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut seen: HashSet<(Vec<u8>, Vec<u8>)> = HashSet::new();

    for index in 0..space {
        let mut rest = index;
        for (mm, slot) in radix.iter_mut().enumerate() {
            *slot = (rest % stride[mm] as u128) as usize;
            rest /= stride[mm] as u128;
        }
        let ca: Vec<u8> = radix
            .iter()
            .zip(ctx.ncol[1].iter())
            .map(|(slot, nb)| (*slot / nb) as u8)
            .collect();
        let cb: Vec<u8> = radix
            .iter()
            .zip(ctx.ncol[1].iter())
            .map(|(slot, nb)| (*slot % nb) as u8)
            .collect();

        let mut total = 0.0f64;
        let mut pruned = false;
        for c in 0..ctx.n {
            total += ctx.best_value(c, &ca, &cb);
            if total + suffix[c + 1] < best - TOL {
                pruned = true;
                break;
            }
        }
        if pruned {
            continue;
        }
        if total > best + TOL {
            best = total;
            assignments.clear();
            seen.clear();
        }
        if (total - best).abs() <= TOL
            && seen.insert((ca.clone(), cb.clone()))
            && assignments.len() < 4096
        {
            assignments.push((ca, cb));
        }
    }

    Outcome { value: best, assignments }
}

/// Notes an assignment, keeping every one that ties with the incumbent.
fn record(
    best: &mut f64,
    assignments: &mut Vec<(Vec<u8>, Vec<u8>)>,
    seen: &mut HashSet<(Vec<u8>, Vec<u8>)>,
    value: f64,
    ca: &[u8],
    cb: &[u8],
) {
    if value > *best + TOL {
        *best = value;
        assignments.clear();
        seen.clear();
    }
    if (value - *best).abs() <= TOL && seen.insert((ca.to_vec(), cb.to_vec())) {
        if assignments.len() < 4096 {
            assignments.push((ca.to_vec(), cb.to_vec()));
        }
    }
}

/// Random restarts with perturbation, keeping every assignment that ties.
fn stochastic(ctx: &Ctx, restarts: usize, kicks: usize) -> Outcome {
    let mut rng = Rng::new(0x9E3779B97F4A7C15);
    let mut best = f64::NEG_INFINITY;
    let mut assignments: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut seen: HashSet<(Vec<u8>, Vec<u8>)> = HashSet::new();

    for round in 0..restarts {
        let mut ca = vec![0u8; ctx.m];
        let mut cb = vec![0u8; ctx.m];
        if round > 0 {
            for mm in 0..ctx.m {
                ca[mm] = rng.below(ctx.ncol[0][mm]) as u8;
                cb[mm] = rng.below(ctx.ncol[1][mm]) as u8;
            }
        }
        let mut value = local_search(ctx, &mut ca, &mut cb);
        record(&mut best, &mut assignments, &mut seen, value, &ca, &cb);

        let mut held_a = ca.clone();
        let mut held_b = cb.clone();
        for _ in 0..kicks {
            let mut trial_a = held_a.clone();
            let mut trial_b = held_b.clone();
            let width = 1 + rng.below(3);
            for _ in 0..width {
                let mm = rng.below(ctx.m);
                if rng.below(2) == 0 {
                    trial_a[mm] = rng.below(ctx.ncol[0][mm]) as u8;
                }
                if rng.below(2) == 0 {
                    trial_b[mm] = rng.below(ctx.ncol[1][mm]) as u8;
                }
            }
            let trial = local_search(ctx, &mut trial_a, &mut trial_b);
            record(&mut best, &mut assignments, &mut seen, trial, &trial_a, &trial_b);
            if trial > value + TOL || (trial - value).abs() <= TOL {
                value = trial;
                held_a.copy_from_slice(&trial_a);
                held_b.copy_from_slice(&trial_b);
            }
        }
    }

    Outcome { value: best, assignments }
}

/// Finds the best total and the assignments that reach it.
pub fn run(ctx: &Ctx) -> Outcome {
    let mut space: u128 = 1;
    for mm in 0..ctx.m {
        space *= (ctx.ncol[0][mm] * ctx.ncol[1][mm]) as u128;
    }
    // A full Viterbi sweep costs roughly `n * m * 4000`; keep enumeration
    // inside a few seconds of work.
    let budget = 3_000_000_000u128 / (ctx.n as u128 * ctx.m as u128 * 4000);
    if space <= budget.max(1) {
        return enumerate(ctx);
    }
    let restarts = 24 + 4096 / (ctx.m * ctx.n).max(1);
    let kicks = 40;
    stochastic(ctx, restarts, kicks)
}
