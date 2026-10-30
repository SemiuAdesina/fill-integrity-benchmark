
//! A beam search over the column of every marker.
//!
//! Where [`crate::solver::exact::Exact`] proves, the beam only proposes: it
//! walks the markers left to right keeping the most promising partial
//! assignments it has seen, ranked by the same [`Relaxed`] bound the exact
//! search prunes with. It cannot prove anything, but it costs a fixed amount of
//! work — one forward pass per child, a few dozen children per marker — and the
//! assignments it ends on are far better starting points than random ones,
//! because the ranking sees every progeny's real forward values at every step.
//!
//! The markers are the chain, so the order is forced: a column can only be
//! judged once the columns before it are fixed, since together they decide what
//! the progeny's paths have cost so far.

use crate::solver::relaxed::{advance, Relaxed};
use crate::solver::tables::{Ctx, NJ, NS};

/// One partial assignment: the forward values of every progeny at the last
/// marker fixed, the columns chosen so far, and how the rows stand.
struct Node {
    fwd: Vec<[f64; NJ]>,
    ca: Vec<u8>,
    cb: Vec<u8>,
    /// Rank of the node, which is the bound on the whole chain given its prefix.
    bound: f64,
    /// What its fixed markers contribute to the shared column bound.
    shared: f64,
    /// Per parent and per pair of rows, whether the pair is still tied (`0`) or
    /// the first has come out above the second (`1`).
    order: [[u8; 6]; 2],
}

impl Node {
    /// The total this node reaches once every marker is fixed: the best value
    /// any path can end in, summed over the progeny.
    fn value(&self) -> f64 {
        let mut total = 0.0f64;
        for row in self.fwd.iter() {
            let mut best = f64::NEG_INFINITY;
            for v in row.iter() {
                if *v > best {
                    best = *v;
                }
            }
            total += best;
        }
        total
    }
}

/// Extends one parent's partial rows by a column, rejecting the branch when two
/// rows have already come out the wrong way round.
fn extend(order: &[u8; 6], mask: u8) -> Option<[u8; 6]> {
    let bits = [
        (mask >> 0) & 1,
        (mask >> 1) & 1,
        (mask >> 2) & 1,
        (mask >> 3) & 1,
    ];
    let mut out = *order;
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

/// Searches with a beam of `width` partial assignments, returning the best
/// `keep` complete ones, most valuable first.
///
/// The width is a ceiling, not a target: symmetry breaking and infeasible
/// children leave fewer, and every child of every node is still examined, so the
/// cost is one forward pass per surviving child.
pub fn beam(ctx: &Ctx, relaxed: &Relaxed, width: usize, keep: usize) -> Vec<(f64, Vec<u8>, Vec<u8>)> {
    if ctx.m == 0 {
        return Vec::new();
    }
    let mut layer: Vec<Node> = vec![Node {
        fwd: (0..ctx.n).map(|_| [f64::NEG_INFINITY; NJ]).collect(),
        ca: vec![0u8; ctx.m],
        cb: vec![0u8; ctx.m],
        bound: relaxed.root_bound(),
        shared: 0.0,
        order: [[0u8; 6]; 2],
    }];

    for k in 0..ctx.m {
        let na = ctx.ncol[0][k];
        let nb = ctx.ncol[1][k];
        let mut children: Vec<Node> = Vec::new();
        for node in layer.iter() {
            for ia in 0..na {
                let order_a = match extend(&node.order[0], ctx.cols[0][k][ia]) {
                    Some(order) => order,
                    None => continue,
                };
                for ib in 0..nb {
                    let order_b = match extend(&node.order[1], ctx.cols[1][k][ib]) {
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
                        advance(ctx, k, c, row, &node.fwd[c], &mut cur);
                        let tail = relaxed.suffix[k + 1][c];
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
                    let shared = node.shared + free_part;
                    let mut ca = node.ca.clone();
                    let mut cb = node.cb.clone();
                    ca[k] = ia as u8;
                    cb[k] = ib as u8;
                    children.push(Node {
                        fwd: next,
                        ca,
                        cb,
                        bound: relaxed.bound(k, paths, shared),
                        shared,
                        order: [order_a, order_b],
                    });
                }
            }
        }
        if children.is_empty() {
            return Vec::new();
        }
        children.sort_by(|x, y| y.bound.partial_cmp(&x.bound).unwrap_or(std::cmp::Ordering::Equal));
        children.truncate(width);
        layer = children;
    }

    let mut done: Vec<(f64, Vec<u8>, Vec<u8>)> =
        layer.iter().map(|node| (node.value(), node.ca.clone(), node.cb.clone())).collect();
    done.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal));
    done.truncate(keep);
    done
}

/// How wide a beam the work budget buys. One child costs a forward pass over
/// every progeny and every joint state, and a marker contributes at most
/// thirty six of them.
pub fn width(ctx: &Ctx) -> usize {
    let per_child = ctx.n as f64 * 2100.0;
    let per_marker = 36.0 * per_child;
    let affordable = 1.5e9 / (ctx.m.max(1) as f64 * per_marker);
    (affordable as usize).clamp(4, 48)
}
