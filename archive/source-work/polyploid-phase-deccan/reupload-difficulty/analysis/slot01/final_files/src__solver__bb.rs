
//! Exact search over parental allele columns.
//!
//! The search branches on the allele column each parent holds at each marker,
//! in marker order. Two things keep it exact and small enough to finish.
//!
//! * A suffix bound. Once markers `0..k` are fixed, every progeny's partial
//!   Viterbi is known exactly up to `k`; what remains can be worth at most what
//!   a chain run against the largest admissible emission at each later marker
//!   would give. Adding those maxima per progeny bounds the whole subtree.
//! * Homolog labels are arbitrary, so only assignments whose four rows are in
//!   ascending order are visited. That is a factor of up to twenty-four, and it
//!   costs three bits of state per parent: whether each adjacent pair of rows
//!   has already been separated by an earlier marker.
//!
//! The Viterbi step into a marker is shared by every column pair tried there,
//! so it is computed once per node and each sibling only pays for its emission.

use crate::cross_input::Cross;
use crate::cross_model::STATES;
use crate::solver::tables::{Tables, JOINTS};
use crate::solver::viterbi::{maxplus, Chain, SuffixBounds};

/// A completed parental assignment the search found worth keeping.
#[derive(Clone)]
pub struct Candidate {
    pub score: f64,
    /// Column index per marker, `(parent A, parent B)`.
    pub picks: Vec<(usize, usize)>,
}

/// Depth first search over parental columns.
pub struct Search<'a> {
    cross: &'a Cross,
    tables: &'a Tables,
    chain: Chain<'a>,
    bounds: &'a SuffixBounds,
    /// `[progeny][marker][joint]` forward vectors along the current branch.
    forward: Vec<Vec<[f64; JOINTS]>>,
    /// Viterbi step into the marker being branched on, per progeny.
    into: Vec<[f64; JOINTS]>,
    picks: Vec<(usize, usize)>,
    /// `u` sums per marker and column pair, `[marker][pair][joint]`.
    u_sum: Vec<Vec<[u8; JOINTS]>>,
    best_score: f64,
    candidates: Vec<Candidate>,
    /// Count of nodes expanded, for diagnostics.
    pub nodes: u64,
    /// Count of leaves scored, for diagnostics.
    pub leaves: u64,
}

impl<'a> Search<'a> {
    pub fn new(cross: &'a Cross, tables: &'a Tables, bounds: &'a SuffixBounds) -> Search<'a> {
        let mut u_sum = Vec::with_capacity(tables.markers);
        for m in 0..tables.markers {
            let na = tables.cols[0][m].len();
            let nb = tables.cols[1][m].len();
            let mut pairs = Vec::with_capacity(na * nb);
            for ia in 0..na {
                let ma = tables.cols[0][m][ia];
                for ib in 0..nb {
                    let mb = tables.cols[1][m][ib];
                    let mut row = [0u8; JOINTS];
                    for sa in 0..STATES {
                        for sb in 0..STATES {
                            row[sa * STATES + sb] = tables.u_of[usize::from(ma)][sa]
                                + tables.u_of[usize::from(mb)][sb];
                        }
                    }
                    pairs.push(row);
                }
            }
            u_sum.push(pairs);
        }

        let offspring = cross.offspring;
        Search {
            cross,
            tables,
            chain: Chain::new(cross, tables),
            bounds,
            forward: vec![vec![[f64::NEG_INFINITY; JOINTS]; tables.markers]; offspring],
            into: vec![[0.0; JOINTS]; offspring],
            picks: vec![(0, 0); tables.markers],
            u_sum,
            best_score: f64::NEG_INFINITY,
            candidates: Vec::new(),
            nodes: 0,
            leaves: 0,
        }
    }

    /// Runs the search. `floor` seeds it with a score already achieved, so
    /// subtrees that cannot beat it are dropped from the start.
    pub fn run(&mut self, floor: f64) -> Vec<Candidate> {
        self.best_score = floor;
        self.candidates.clear();
        self.descend(0, 0, 0);
        self.retain_best();
        std::mem::take(&mut self.candidates)
    }

    /// Drops candidates that fell below the best once it improved.
    fn retain_best(&mut self) {
        let limit = self.best_score - TOLERANCE;
        self.candidates.retain(|c| c.score >= limit);
    }

    /// Whether the column `mask` keeps `parent`'s rows ascending, given which
    /// adjacent pairs are still tied; returns the updated state.
    fn row_state(mask: u8, state: u8) -> Option<u8> {
        let mut out = state;
        for pair in 0..3u8 {
            if out & (1u8 << pair) != 0 {
                continue;
            }
            let low = (mask >> pair) & 1;
            let high = (mask >> (pair + 1)) & 1;
            if low > high {
                return None;
            }
            if low < high {
                out |= 1u8 << pair;
            }
        }
        Some(out)
    }

    fn descend(&mut self, marker: usize, state_a: u8, state_b: u8) {
        let markers = self.tables.markers;
        self.nodes += 1;

        if marker > 0 {
            let ta = &self.tables.tmat[0][marker - 1];
            let tb = &self.tables.tmat[1][marker - 1];
            for c in 0..self.cross.offspring {
                let mut into = [f64::NEG_INFINITY; JOINTS];
                maxplus(&self.forward[c][marker - 1], ta, tb, &mut into);
                self.into[c] = into;
            }
        } else {
            for c in 0..self.cross.offspring {
                self.into[c] = [0.0; JOINTS];
            }
        }

        let last = marker + 1 == markers;
        let na = self.tables.cols[0][marker].len();
        let nb = self.tables.cols[1][marker].len();
        let drj = &self.tables.drj[marker];
        let legal = &self.tables.legj[marker];
        let width = nb;

        for ia in 0..na {
            let mask_a = self.tables.cols[0][marker][ia];
            let next_a = match Search::row_state(mask_a, state_a) {
                Some(s) => s,
                None => continue,
            };
            for ib in 0..nb {
                let mask_b = self.tables.cols[1][marker][ib];
                let next_b = match Search::row_state(mask_b, state_b) {
                    Some(s) => s,
                    None => continue,
                };
                let u_row = &self.u_sum[marker][ia * width + ib];

                let mut bound = 0.0f64;
                let mut exhausted = false;
                for c in 0..self.cross.offspring {
                    let gl = self.cross.child_gl[c][marker];
                    let into = &self.into[c];
                    let out = &mut self.forward[c][marker];
                    let cont = if last {
                        None
                    } else {
                        Some(&self.bounds.cont[c][marker + 1])
                    };
                    let mut best = f64::NEG_INFINITY;
                    for j in 0..JOINTS {
                        if !legal[j] {
                            out[j] = f64::NEG_INFINITY;
                            continue;
                        }
                        let value = into[j] + gl[usize::from(u_row[j])] + drj[j];
                        out[j] = value;
                        let total = match cont {
                            Some(cont) => value + cont[j],
                            None => value,
                        };
                        if total > best {
                            best = total;
                        }
                    }
                    if best == f64::NEG_INFINITY {
                        exhausted = true;
                        break;
                    }
                    bound += best;
                    if bound + self.bounds_total_above(c, marker) < self.best_score - TOLERANCE {
                        exhausted = true;
                        break;
                    }
                }

                if exhausted {
                    continue;
                }
                if bound < self.best_score - TOLERANCE {
                    continue;
                }

                self.picks[marker] = (ia, ib);
                if last {
                    self.leaves += 1;
                    if bound > self.best_score {
                        self.best_score = bound;
                        self.retain_best();
                    }
                    if bound >= self.best_score - TOLERANCE {
                        self.candidates.push(Candidate {
                            score: bound,
                            picks: self.picks.clone(),
                        });
                    }
                } else {
                    self.descend(marker + 1, next_a, next_b);
                }
            }
        }
    }

    /// Largest the still unaccounted progeny can add, used to abandon a column
    /// pair part way through.
    fn bounds_total_above(&self, progeny: usize, marker: usize) -> f64 {
        let mut sum = 0.0;
        for c in progeny + 1..self.cross.offspring {
            sum += self.bounds.total[c];
        }
        let _ = marker;
        sum
    }

    /// Best score found.
    pub fn best_score(&self) -> f64 {
        self.best_score
    }

    /// Unused marker-independent helper: the emission table of a column pair.
    pub fn emission_of(&self, progeny: usize, marker: usize, ia: usize, ib: usize) -> [f64; JOINTS] {
        let mut out = [f64::NEG_INFINITY; JOINTS];
        let mask_a = self.tables.cols[0][marker][ia];
        let mask_b = self.tables.cols[1][marker][ib];
        self.chain
            .emission(progeny, marker, mask_a, mask_b, &mut out);
        out
    }
}

/// Scores this close to the best are the same score, per the specification.
pub const TOLERANCE: f64 = 1e-8;

/// How many nodes the search may expand before it gives up.
///
/// A node costs work proportional to the number of progeny, so a fixed node
/// count would cost eight times as long on thirty two progeny as on four. The
/// budget is therefore divided by the progeny count, which keeps the search to a
/// bounded number of seconds anywhere in the declared range. It depends only on
/// the cross, never on the clock, so that repeated runs agree.
fn budget_of(offspring: usize) -> u64 {
    let scaled = 20_000_000u64 / offspring as u64;
    scaled.clamp(200_000, 1_500_000)
}

/// The order to try each marker's surviving pairs in: the pair `guide` holds at
/// that marker first, then the rest in the order they were given.
///
/// The search goes depth first, so the first pair it takes at every marker
/// decides where the first dive ends up. Left to itself that dive can spend the
/// whole budget in the middle of a long chain without ever reaching a leaf;
/// pointing it at the best assignment known makes the first dive end at a real
/// score, and every later dive is a variation on one.
fn guide_order(reduced: &Reduced, guide: &[(usize, usize)]) -> Vec<Vec<usize>> {
    reduced
        .pairs
        .iter()
        .enumerate()
        .map(|(m, pairs)| {
            let lead = pairs.iter().position(|pair| *pair == guide[m]);
            let mut order: Vec<usize> = (0..pairs.len()).collect();
            if let Some(at) = lead {
                order.remove(at);
                order.insert(0, at);
            }
            order
        })
        .collect()
}
