
//! A feasible starting point for the search.
//!
//! The bounds can only prune against an incumbent, and the first complete
//! assignment the descent happens to reach can sit several nats below the
//! optimum, which leaves the bounds nothing to cut with. This module finds a
//! strong feasible assignment first, by ordinary local search over the parental
//! columns, and hands its exact score to the search as the initial incumbent.
//!
//! Nothing here decides the answer. The incumbent is an admissible assignment
//! with an exactly computed score, so pruning against it is as safe as pruning
//! against any leaf the search itself records, and the search still explores
//! every branch that can tie it. If the local search happens to find the
//! optimum, the search proves it; if it does not, the search finds it.
//!
//! Determinism: the search below uses a fixed-seed generator and fixed tie
//! orders, so repeated runs take identical paths.

use crate::cross_input::Cross;

use super::columns::Column;
use super::progeny_dp::{emission, StaticTerms, JOINT};

/// One parent's column and the other's at one marker.
pub type Pair = (Column, Column);

/// A feasible column assignment and its exact total.
pub struct WarmStart {
    pub columns_a: Vec<Column>,
    pub columns_b: Vec<Column>,
    pub total: f64,
    /// How many local moves were accepted, for the tractability record.
    pub moves: u64,
}

/// Small deterministic generator. No third party crates are available and
/// nothing about the answer depends on the sequence, only the order in which
/// starting points are tried.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % (n as u64)) as usize
    }
}

/// Forward and backward tables for every progeny under one fixed assignment.
struct Tables {
    /// `pre[c][m][j]`: best score of markers `0..m` arriving in state `j` at
    /// `m`, before `m`'s own node term. Zero at the first marker.
    pre: Vec<Vec<Vec<f64>>>,
    /// `post[c][m][j]`: best score of markers `m+1..` leaving state `j` at `m`.
    post: Vec<Vec<Vec<f64>>>,
    total: f64,
}

pub(super) fn node_term(
    cross: &Cross,
    terms: &StaticTerms,
    c: usize,
    m: usize,
    j: usize,
    pair: &Pair,
) -> f64 {
    let dr = terms.dr_at(m)[j];
    if dr == f64::NEG_INFINITY {
        return f64::NEG_INFINITY;
    }
    emission(cross, c, m, j, &pair.0, &pair.1) + dr
}

impl Tables {
    fn build(cross: &Cross, terms: &StaticTerms, assign: &[Pair]) -> Tables {
        let markers = cross.markers;
        let mut pre = Vec::with_capacity(cross.offspring);
        let mut post = Vec::with_capacity(cross.offspring);
        let mut total = 0.0;
        for c in 0..cross.offspring {
            let mut fwd = vec![vec![f64::NEG_INFINITY; JOINT]; markers];
            let mut pre_c = vec![vec![0.0f64; JOINT]; markers];
            for m in 0..markers {
                if m > 0 {
                    let mut out = vec![f64::NEG_INFINITY; JOINT];
                    terms.advance(m - 1, &fwd[m - 1], &mut out);
                    pre_c[m] = out;
                }
                for j in 0..JOINT {
                    let node = node_term(cross, terms, c, m, j, &assign[m]);
                    fwd[m][j] = if node == f64::NEG_INFINITY || pre_c[m][j] == f64::NEG_INFINITY {
                        f64::NEG_INFINITY
                    } else {
                        pre_c[m][j] + node
                    };
                }
            }
            let mut post_c = vec![vec![0.0f64; JOINT]; markers];
            for m in (0..markers.saturating_sub(1)).rev() {
                let mut future = vec![f64::NEG_INFINITY; JOINT];
                for j in 0..JOINT {
                    let node = node_term(cross, terms, c, m + 1, j, &assign[m + 1]);
                    if node != f64::NEG_INFINITY && post_c[m + 1][j] != f64::NEG_INFINITY {
                        future[j] = node + post_c[m + 1][j];
                    }
                }
                let mut carried = vec![f64::NEG_INFINITY; JOINT];
                terms.advance_backward(m, &future, &mut carried);
                post_c[m] = carried;
            }
            let best = fwd[markers - 1].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            total += best;
            pre.push(pre_c);
            post.push(post_c);
        }
        Tables { pre, post, total }
    }

    /// Exact total if marker `m` used `pair` and every other marker kept its
    /// current columns.
    fn with_pair(&self, cross: &Cross, terms: &StaticTerms, m: usize, pair: &Pair) -> f64 {
        let mut total = 0.0;
        for c in 0..cross.offspring {
            let mut best = f64::NEG_INFINITY;
            let pre = &self.pre[c][m];
            let post = &self.post[c][m];
            for j in 0..JOINT {
                if pre[j] == f64::NEG_INFINITY || post[j] == f64::NEG_INFINITY {
                    continue;
                }
                let node = node_term(cross, terms, c, m, j, pair);
                if node == f64::NEG_INFINITY {
                    continue;
                }
                let v = pre[j] + node + post[j];
                if v > best {
                    best = v;
                }
            }
            if best == f64::NEG_INFINITY {
                return f64::NEG_INFINITY;
            }
            total += best;
        }
        total
    }
}

/// Exact total of an assignment, computed from scratch.
pub(super) fn evaluate(cross: &Cross, terms: &StaticTerms, assign: &[Pair]) -> f64 {
    Tables::build(cross, terms, assign).total
}

/// Steepest ascent over single-marker column changes until no change helps.
fn climb(
    cross: &Cross,
    terms: &StaticTerms,
    pairs: &[Vec<Pair>],
    assign: &mut Vec<Pair>,
    moves: &mut u64,
) -> f64 {
    let mut tables = Tables::build(cross, terms, assign);
    loop {
        let mut best_gain = 1e-9;
        let mut best_move: Option<(usize, Pair)> = None;
        for m in 0..cross.markers {
            for pair in &pairs[m] {
                if *pair == assign[m] {
                    continue;
                }
                let t = tables.with_pair(cross, terms, m, pair);
                if t - tables.total > best_gain {
                    best_gain = t - tables.total;
                    best_move = Some((m, *pair));
                }
            }
        }
        let Some((m, pair)) = best_move else {
            return tables.total;
        };
        assign[m] = pair;
        *moves += 1;
        tables = Tables::build(cross, terms, assign);
    }
}

/// Swaps two homolog rows of one parent from marker `from` to the end.
///
/// This is the move that a single-marker change cannot make: a phase switch
/// across an interval, which changes every column where the two rows differ
/// and re-routes every progeny that inherited either row. Applying it to an
/// admissible assignment gives an admissible assignment, because each column
/// keeps its dosage.
fn phase_switch(assign: &mut [Pair], parent: usize, h1: usize, h2: usize, from: usize) {
    for pair in assign.iter_mut().skip(from) {
        let col = if parent == 0 { &mut pair.0 } else { &mut pair.1 };
        col.swap(h1, h2);
    }
}

/// Applies the best improving phase switch, then climbs, until none improves.
fn switch_polish(
    cross: &Cross,
    terms: &StaticTerms,
    pairs: &[Vec<Pair>],
    assign: &mut Vec<Pair>,
    mut current: f64,
    moves: &mut u64,
) -> f64 {
    let markers = cross.markers;
    loop {
        let mut best_switch: Option<(f64, Vec<Pair>)> = None;
        for parent in 0..2 {
            for h1 in 0..4 {
                for h2 in (h1 + 1)..4 {
                    for from in 1..markers {
                        let mut trial = assign.clone();
                        phase_switch(&mut trial, parent, h1, h2, from);
                        if trial == *assign || (0..markers).any(|m| !pairs[m].contains(&trial[m])) {
                            continue;
                        }
                        let t = evaluate(cross, terms, &trial);
                        if t > current + 1e-9 && best_switch.as_ref().map_or(true, |(b, _)| t > *b) {
                            best_switch = Some((t, trial));
                        }
                    }
                }
            }
        }
        let Some((t, trial)) = best_switch else {
            return current;
        };
        *assign = trial;
        *moves += 1;
        current = climb(cross, terms, pairs, assign, moves).max(t);
    }
}

/// Local search from several starting points; returns the best assignment.
pub fn warm_start(cross: &Cross, terms: &StaticTerms, pairs: &[Vec<Pair>]) -> WarmStart {
    let markers = cross.markers;
    let mut moves = 0u64;

    // Start one: each marker's best column pair on its own, the marker at a
    // time reconstruction. Wrong as an answer, useful as a start.
    let mut greedy: Vec<Pair> = Vec::with_capacity(markers);
    for m in 0..markers {
        let mut best = (f64::NEG_INFINITY, pairs[m][0]);
        for pair in &pairs[m] {
            let mut total = 0.0;
            for c in 0..cross.offspring {
                let mut b = f64::NEG_INFINITY;
                for j in 0..JOINT {
                    let v = node_term(cross, terms, c, m, j, pair);
                    if v > b {
                        b = v;
                    }
                }
                total += b;
            }
            if total > best.0 {
                best = (total, *pair);
            }
        }
        greedy.push(best.1);
    }

    let mut best_assign = greedy.clone();
    let mut best_total = climb(cross, terms, pairs, &mut best_assign, &mut moves);
    best_total = switch_polish(cross, terms, pairs, &mut best_assign, best_total, &mut moves);
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);

    let restarts = 24usize;
    for round in 0..restarts {
        let mut assign = best_assign.clone();
        if round % 4 == 3 {
            // A fresh random start every fourth round, so the search is not
            // confined to the neighbourhood of the greedy start.
            for m in 0..markers {
                assign[m] = pairs[m][rng.below(pairs[m].len())];
            }
        } else {
            // Otherwise a kick: a phase switch, then two random column changes.
            let parent = rng.below(2);
            let h1 = rng.below(4);
            let mut h2 = rng.below(4);
            while h2 == h1 {
                h2 = rng.below(4);
            }
            let from = rng.below(markers);
            phase_switch(&mut assign, parent, h1, h2, from);
            for _ in 0..2 {
                let m = rng.below(markers);
                assign[m] = pairs[m][rng.below(pairs[m].len())];
            }
            for m in 0..markers {
                if !pairs[m].contains(&assign[m]) {
                    assign[m] = pairs[m][rng.below(pairs[m].len())];
                }
            }
        }
        let total = climb(cross, terms, pairs, &mut assign, &mut moves);
        if total > best_total + 1e-9 {
            best_total = total;
            best_assign = assign;
        }
    }
    best_total = switch_polish(cross, terms, pairs, &mut best_assign, best_total, &mut moves);
    let _ = best_total;

    // Canonical row order within the orbit, so the incumbent is the smallest
    // member of its relabelling class under the result format's order.
    let (columns_a, columns_b) = sort_rows(&best_assign);
    let sorted: Vec<Pair> = columns_a.iter().cloned().zip(columns_b.iter().cloned()).collect();
    let total = evaluate(cross, terms, &sorted);
    WarmStart { columns_a, columns_b, total, moves }
}

/// Reorders each parent's four rows ascending, which preserves the score.
fn sort_rows(assign: &[Pair]) -> (Vec<Column>, Vec<Column>) {
    let markers = assign.len();
    let mut out = (Vec::with_capacity(markers), Vec::with_capacity(markers));
    for parent in 0..2 {
        let mut rows: Vec<Vec<u8>> = (0..4)
            .map(|h| {
                (0..markers)
                    .map(|m| if parent == 0 { assign[m].0[h] } else { assign[m].1[h] })
                    .collect()
            })
            .collect();
        rows.sort();
        for m in 0..markers {
            let col = [rows[0][m], rows[1][m], rows[2][m], rows[3][m]];
            if parent == 0 {
                out.0.push(col)
            } else {
                out.1.push(col)
            }
        }
    }
    out
}
