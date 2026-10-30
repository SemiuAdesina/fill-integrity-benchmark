
//! Thinning the columns a marker admits before the search branches on them.
//!
//! The search over parental columns is exact but its bound is loose, because the
//! bound lets every progeny pick its own columns. That looseness is what makes a
//! large cross slow. This module removes it in a different way: for each marker
//! and each column pair it asks how good the best answer that uses that pair
//! could possibly be, with every other marker still relaxed. Any pair whose
//! ceiling falls below a score already achieved cannot appear in any optimal
//! answer, so it is struck out.
//!
//! Striking a pair lowers the ceiling on the emission the others compete for, so
//! the whole thing is repeated until nothing moves. How much that settles depends
//! entirely on the cross: where one marker's evidence is clear it leaves a single
//! pair and the search never has to branch there, and where every marker is
//! nearly ambiguous it leaves them all, because the bound it reasons from is the
//! same relaxed one the search uses and inherits its slack.
//!
//! Each round is cheap: two sweeps of the chain, and then one pass over the
//! surviving pairs of each marker, which the forward and backward vectors already
//! computed answer in a hundred operations per progeny.

use crate::cross_input::Cross;
use crate::solver::tables::{Tables, JOINTS};
use crate::solver::viterbi::SuffixBounds;

/// Scores this close to the best are the same score, per the specification.
pub const TOLERANCE: f64 = 1e-8;

/// How many rounds of striking out to run at most.
const ROUNDS: usize = 64;

/// The column pairs the search still has to consider, and the emission ceiling
/// they imply.
pub struct Reduced {
    /// Surviving pairs per marker, as column indices `(parent A, parent B)`.
    pub pairs: Vec<Vec<(usize, usize)>>,
    /// `[progeny][marker][joint]` largest emission a surviving pair reaches.
    pub ub: Vec<Vec<[f64; JOINTS]>>,
}

impl Reduced {
    /// Whether every marker is down to a single pair, in which case the
    /// haplotypes are decided and there is nothing to search.
    pub fn decided(&self) -> bool {
        self.pairs.iter().all(|m| m.len() == 1)
    }
}

/// Strikes out every pair that cannot reach `lower`, and returns what is left.
///
/// `lower` is a score already achieved. The result is valid for any `lower` that
/// is at most the optimum: pairs are only removed when no answer reaching the
/// optimum can use them.
pub fn reduce(cross: &Cross, tables: &Tables, lower: f64) -> Reduced {
    let markers = cross.markers;
    let offspring = cross.offspring;

    let mut alive: Vec<Vec<bool>> = (0..markers)
        .map(|m| vec![true; tables.cols[0][m].len() * tables.cols[1][m].len()])
        .collect();
    let mut ub = ceiling(cross, tables, &alive);

    for _round in 0..ROUNDS {
        let bounds = SuffixBounds::build(cross, tables, &ub);
        let mut changed = false;

        for m in 0..markers {
            let mut best = f64::NEG_INFINITY;
            let mut best_pair = 0usize;
            for (pair, keep) in alive[m].iter_mut().enumerate() {
                if !*keep {
                    continue;
                }
                let row = &tables.u_sum[m][pair];
                let mut ceiling_total = 0.0f64;
                for c in 0..offspring {
                    let gl = cross.child_gl[c][m];
                    let through = &bounds.fwd[c][m];
                    let after = if m + 1 < markers {
                        &bounds.cont[c][m + 1]
                    } else {
                        &ZERO
                    };
                    let mut best_here = f64::NEG_INFINITY;
                    for j in 0..JOINTS {
                        if !tables.legj[m][j] || through[j] == f64::NEG_INFINITY {
                            continue;
                        }
                        let emission = gl[usize::from(row[j])] + tables.drj[m][j];
                        let total = through[j] - ub[c][m][j] + emission + after[j];
                        if total > best_here {
                            best_here = total;
                        }
                    }
                    ceiling_total += best_here;
                }
                if ceiling_total > best {
                    best = ceiling_total;
                    best_pair = pair;
                }
                if ceiling_total < lower - TOLERANCE {
                    *keep = false;
                    changed = true;
                }
            }
            // The score that justified `lower` came from some real assignment,
            // so a marker cannot really lose every pair. Should rounding ever
            // empty one out, keep its best rather than fail.
            if !alive[m].iter().any(|k| *k) {
                alive[m][best_pair] = true;
            }
        }

        if !changed {
            break;
        }
        ub = ceiling(cross, tables, &alive);
    }

    let mut pairs = Vec::with_capacity(markers);
    for m in 0..markers {
        let nb = tables.cols[1][m].len();
        let mut list = Vec::new();
        for (pair, keep) in alive[m].iter().enumerate() {
            if *keep {
                list.push((pair / nb, pair % nb));
            }
        }
        // The most promising pair first. The search dives to the bottom of the
        // tree before it comes back up, so the order it tries the columns of a
        // marker decides which score it meets first; a good one early raises the
        // floor the rest have to beat. Sorting is stable, so pairs that look
        // equally good stay in column order and the search is reproducible.
        let width = tables.cols[1][m].len();
        list.sort_by(|left, right| {
            let l = appeal(cross, tables, m, left.0 * width + left.1);
            let r = appeal(cross, tables, m, right.0 * width + right.1);
            r.partial_cmp(&l).unwrap_or(std::cmp::Ordering::Equal)
        });
        pairs.push(list);
    }

    Reduced { pairs, ub }
}

/// How much every progeny together likes the column pair `pair` at `marker`:
/// the best any of them could score there. Only the ranking is used.
fn appeal(cross: &Cross, tables: &Tables, marker: usize, pair: usize) -> f64 {
    let row = &tables.u_sum[marker][pair];
    let mut sum = 0.0f64;
    for c in 0..cross.offspring {
        let gl = cross.child_gl[c][marker];
        let mut best = f64::NEG_INFINITY;
        for j in 0..JOINTS {
            if tables.legj[marker][j] {
                best = best.max(gl[usize::from(row[j])]);
            }
        }
        sum += best;
    }
    sum
}

/// The nothing that follows the last marker, as a vector of zeros.
const ZERO: [f64; JOINTS] = [0.0; JOINTS];

/// `[progeny][marker][joint]` largest emission the surviving pairs reach.
fn ceiling(cross: &Cross, tables: &Tables, alive: &[Vec<bool>]) -> Vec<Vec<[f64; JOINTS]>> {
    let markers = cross.markers;
    let mut out = Vec::with_capacity(cross.offspring);
    for c in 0..cross.offspring {
        let mut rows = Vec::with_capacity(markers);
        for m in 0..markers {
            let mut row = [f64::NEG_INFINITY; JOINTS];
            for (pair, keep) in alive[m].iter().enumerate() {
                if !*keep {
                    continue;
                }
                let u_row = &tables.u_sum[m][pair];
                let gl = cross.child_gl[c][m];
                for j in 0..JOINTS {
                    if !tables.legj[m][j] {
                        continue;
                    }
                    let emission = gl[usize::from(u_row[j])];
                    if emission > row[j] {
                        row[j] = emission;
                    }
                }
            }
            rows.push(row);
        }
        out.push(rows);
    }
    out
}
