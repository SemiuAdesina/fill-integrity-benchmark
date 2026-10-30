
//! Bounds on the part of the marker chain that is not yet fixed.
//!
//! A search that decides the columns one marker at a time has, at every step, a
//! prefix it can score exactly and a suffix it can only bound. Two bounds are
//! used here, and the smaller of the two taken, because they relax different
//! things and neither is always the tighter:
//!
//! * `suffix` keeps every progeny's real Viterbi path and the real transition
//!   terms, but lets each progeny choose its own columns at the markers that are
//!   still free. It ignores that the progeny have to agree on them, which is
//!   most of what makes the problem hard.
//! * `free` scores every remaining marker on its own, with its column shared by
//!   all progeny as it really will be, and takes every transition at its
//!   largest. It ignores that the states have to form a path.
//!
//! Both are precomputed once, before any search: `suffix` costs one backward
//! sweep per progeny, `free` one pass over the admissible column pairs.

use crate::solver::tables::{Ctx, NJ, NS};

/// The two relaxations, and the shared step that uses them.
pub struct Relaxed {
    /// `suffix[k][c][u]`: the most progeny `c` can still contribute from marker
    /// `k` onwards, given that it stands in joint state `u` at marker `k - 1`.
    /// Slot `0` holds a whole chain value, the same in every state.
    pub suffix: Vec<Vec<[f64; NJ]>>,
    /// Suffix sums over the markers of `free_max`.
    pub free_suffix: Vec<f64>,
    /// Every progeny's largest transition term, summed over every interval.
    pub trans_total: f64,
    /// Number of progeny the bounds were built for.
    pub n: usize,
}

impl Relaxed {
    /// Precomputes both bounds.
    pub fn new(ctx: &Ctx) -> Relaxed {
        // Every dosage a joint state can reach at a marker, over the admissible
        // column pairs, as a five bit mask.
        let mut reach: Vec<[u8; NJ]> = vec![[0u8; NJ]; ctx.m];
        for mm in 0..ctx.m {
            for row in ctx.dose[mm].iter() {
                for j in 0..NJ {
                    reach[mm][j] |= 1u8 << row[j];
                }
            }
        }

        let mut suffix: Vec<Vec<[f64; NJ]>> = vec![vec![[0.0f64; NJ]; ctx.n]; ctx.m + 1];
        for c in 0..ctx.n {
            let mut node: Vec<[f64; NJ]> = Vec::with_capacity(ctx.m);
            for mm in 0..ctx.m {
                let mut row = [f64::NEG_INFINITY; NJ];
                for j in 0..NJ {
                    let mut best = f64::NEG_INFINITY;
                    for d in 0..5u8 {
                        if reach[mm][j] & (1 << d) != 0 && ctx.gl[c][mm][d as usize] > best {
                            best = ctx.gl[c][mm][d as usize];
                        }
                    }
                    row[j] = best + ctx.drv[0][mm][j / NS] + ctx.drv[1][mm][j % NS];
                }
                node.push(row);
            }
            for k in (1..ctx.m).rev() {
                let later = suffix[k + 1][c];
                let ta = &ctx.tr[0][k - 1];
                let tb = &ctx.tr[1][k - 1];
                let here = node[k];
                let mut row = [f64::NEG_INFINITY; NJ];
                for u in 0..NJ {
                    let sa = u / NS;
                    let sb = u % NS;
                    let mut best = f64::NEG_INFINITY;
                    for sa2 in 0..NS {
                        for sb2 in 0..NS {
                            let v = ta[sa * NS + sa2]
                                + tb[sb * NS + sb2]
                                + here[sa2 * NS + sb2]
                                + later[sa2 * NS + sb2];
                            if v > best {
                                best = v;
                            }
                        }
                    }
                    row[u] = best;
                }
                suffix[k][c] = row;
            }
            let mut total = f64::NEG_INFINITY;
            for u in 0..NJ {
                let v = node[0][u] + suffix[1][c][u];
                if v > total {
                    total = v;
                }
            }
            suffix[0][c] = [total; NJ];
        }

        let mut free_max = Vec::with_capacity(ctx.m);
        for mm in 0..ctx.m {
            let na = ctx.ncol[0][mm];
            let nb = ctx.ncol[1][mm];
            let mut best = f64::NEG_INFINITY;
            for ia in 0..na {
                for ib in 0..nb {
                    let row = &ctx.dose[mm][ia * nb + ib];
                    let mut sum = 0.0f64;
                    for c in 0..ctx.n {
                        let mut best_state = f64::NEG_INFINITY;
                        for j in 0..NJ {
                            let v = ctx.gl[c][mm][row[j] as usize]
                                + ctx.drv[0][mm][j / NS]
                                + ctx.drv[1][mm][j % NS];
                            if v > best_state {
                                best_state = v;
                            }
                        }
                        sum += best_state;
                    }
                    if sum > best {
                        best = sum;
                    }
                }
            }
            free_max.push(best);
        }
        let mut free_suffix = vec![0.0f64; ctx.m + 1];
        for mm in (0..ctx.m).rev() {
            free_suffix[mm] = free_suffix[mm + 1] + free_max[mm];
        }

        let mut trans = 0.0f64;
        for iv in 0..ctx.m - 1 {
            for pi in 0..2 {
                let mut best = f64::NEG_INFINITY;
                for v in ctx.tr[pi][iv].iter() {
                    if *v > best {
                        best = *v;
                    }
                }
                trans += best;
            }
        }

        Relaxed { suffix, free_suffix, trans_total: trans * ctx.n as f64, n: ctx.n }
    }

    /// The bound on a chain in which nothing is fixed yet.
    pub fn root_bound(&self) -> f64 {
        (0..self.n).map(|c| self.suffix[0][c][0]).sum()
    }

    /// The smaller of the two bounds on a chain whose markers `0..=k` are fixed.
    ///
    /// `paths` is the first relaxation already summed over the progeny, and
    /// `shared` is what the fixed markers contribute to the second one, marker
    /// `k` included.
    pub fn bound(&self, k: usize, paths: f64, shared: f64) -> f64 {
        let rest = shared + self.free_suffix[k + 1] + self.trans_total;
        if rest < paths {
            rest
        } else {
            paths
        }
    }
}

/// Advances every progeny's forward values over one marker.
///
/// `row` is the dosage of every joint state at marker `k` under the column pair
/// being tried, `prev` the values at marker `k - 1` (unused when `k` is `0`),
/// and `cur` where the new values are written. Both parents' transitions are
/// applied, one parent at a time, so the cost is quadratic in the number of
/// states per parent rather than quartic in the joint number.
pub fn advance(ctx: &Ctx, k: usize, c: usize, row: &[u8; NJ], prev: &[f64; NJ], cur: &mut [f64; NJ]) {
    if k == 0 {
        for j in 0..NJ {
            cur[j] = ctx.gl[c][0][row[j] as usize]
                + ctx.drv[0][0][j / NS]
                + ctx.drv[1][0][j % NS];
        }
        return;
    }
    let ta = &ctx.tr[0][k - 1];
    let tb = &ctx.tr[1][k - 1];
    let mut mid = [f64::NEG_INFINITY; NJ];
    for sa in 0..NS {
        for sb in 0..NS {
            let mut best = f64::NEG_INFINITY;
            for p in 0..NS {
                let v = prev[sa * NS + p] + tb[p * NS + sb];
                if v > best {
                    best = v;
                }
            }
            mid[sa * NS + sb] = best;
        }
    }
    for sb in 0..NS {
        for sa in 0..NS {
            let mut best = f64::NEG_INFINITY;
            for p in 0..NS {
                let v = mid[p * NS + sb] + ta[p * NS + sa];
                if v > best {
                    best = v;
                }
            }
            cur[sa * NS + sb] = best
                + ctx.gl[c][k][row[sa * NS + sb] as usize]
                + ctx.drv[0][k][sa]
                + ctx.drv[1][k][sb];
        }
    }
}
