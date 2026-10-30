
//! Branch and bound over the shared parental allele columns.
//!
//! The progeny are conditionally independent given the parental haplotypes, but
//! they all share those haplotypes, so the columns cannot be chosen one progeny
//! at a time. The search therefore branches on columns, marker by marker, and
//! carries every progeny's arrival scores forward together.
//!
//! At each node the exact score of the markers decided so far is known, and the
//! bounds in `super::bounds` supply an admissible upper bound on everything
//! still to come. A node whose optimistic total cannot beat the best complete
//! answer found so far is discarded whole. Each bound relaxes a constraint the
//! real problem imposes, so neither ever understates what is reachable and
//! neither prunes the optimum away.

use crate::cross_input::Cross;
use crate::cross_model::HOMOLOGS;

use super::bounds::{MarkerBound, RemainingBound};
use super::pair_bound::PairBound;
use super::columns::{Column, RowOrder};
use super::warm_start::{Pair, WarmStart};
use super::progeny_dp::{StaticTerms, JOINT};

/// A completed column assignment and the score it achieves.
pub struct BestAssignment {
    pub columns_a: Vec<Column>,
    pub columns_b: Vec<Column>,
    pub total: f64,
    /// What the search cost to prove this assignment optimal.
    pub stats: SearchStats,
}

/// What the search did, for the tractability record in the build tooling.
///
/// Counted rather than estimated, because the point of recording them is to show
/// how far the bounds actually cut the tree rather than how far they should.
#[derive(Clone, Copy)]
pub struct SearchStats {
    /// Legal parental column pairs across all markers: the width of the tree at
    /// each level multiplied out, which is the space the bounds work against.
    pub legal_states: u128,
    /// Nodes whose subtree was entered.
    pub expanded: u64,
    /// Nodes discarded by a bound, by an infeasible progeny, or by the homolog
    /// symmetry reduction, and so never entered.
    pub pruned: u64,
    /// Author-side instrumentation: score of the first complete assignment reached.
    pub first_leaf: f64,
    /// Author-side instrumentation: optimistic bound of the best root child.
    pub root_bound: f64,
    /// Author-side instrumentation: number of complete assignments recorded (ties incl.).
    pub leaves: u64,
    /// Author-side instrumentation: number of incumbent replacements at equal score.
    pub tie_replacements: u64,
    /// Author-side instrumentation: distinct complete assignments attaining the
    /// final optimum. One means the optimum is unique.
    pub distinct_optima: u64,
}

impl Default for SearchStats {
    fn default() -> Self {
        SearchStats { legal_states: 0, expanded: 0, pruned: 0, first_leaf: f64::NAN, root_bound: f64::NAN, leaves: 0, tie_replacements: 0, distinct_optima: 0 }
    }
}

/// State carried down one branch of the search.
struct Frontier {
    /// Arrival scores per progeny at the marker just decided, `[progeny][joint]`.
    scores: Vec<Vec<f64>>,
    /// Exact total over the markers decided so far.
    exact: f64,
}

pub struct Search<'a> {
    cross: &'a Cross,
    terms: &'a StaticTerms,
    bounds: &'a [RemainingBound],
    marker_bound: &'a MarkerBound,
    pair_bound: &'a PairBound,
    pairs: &'a [Vec<Pair>],
    chosen_a: Vec<Column>,
    chosen_b: Vec<Column>,
    best: Option<BestAssignment>,
    /// Author-side instrumentation: assignments tying the current best.
    tied: Vec<(Vec<Column>, Vec<Column>)>,
    stats: SearchStats,
}

impl<'a> Search<'a> {
    pub fn new(
        cross: &'a Cross,
        terms: &'a StaticTerms,
        bounds: &'a [RemainingBound],
        marker_bound: &'a MarkerBound,
        pair_bound: &'a PairBound,
        pairs: &'a [Vec<Pair>],
        warm: &WarmStart,
    ) -> Search<'a> {
        let legal_states = pairs.iter().map(|p| p.len() as u128).product();
        let best = Some(BestAssignment {
            columns_a: warm.columns_a.clone(),
            columns_b: warm.columns_b.clone(),
            total: warm.total,
            stats: SearchStats::default(),
        });
        Search {
            cross,
            terms,
            bounds,
            marker_bound,
            pair_bound,
            pairs,
            chosen_a: Vec::with_capacity(cross.markers),
            chosen_b: Vec::with_capacity(cross.markers),
            best,
            tied: Vec::new(),
            stats: SearchStats { legal_states, ..SearchStats::default() },
        }
    }

    pub fn run(mut self) -> BestAssignment {
        let start = Frontier {
            scores: vec![vec![f64::NEG_INFINITY; JOINT]; self.cross.offspring],
            exact: 0.0,
        };
        self.descend(0, &start, RowOrder::new(), RowOrder::new());
        let mut stats = self.stats;
        stats.distinct_optima = self.tied.len() as u64;
        let mut best =
            self.best.expect("the dosage constraint always admits at least one assignment");
        best.stats = stats;
        best
    }

    fn descend(&mut self, marker: usize, frontier: &Frontier, order_a: RowOrder, order_b: RowOrder) {
        if marker == self.cross.markers {
            self.record(frontier);
            return;
        }

        // Propagating the previous marker's scores forward does not depend on
        // which columns are chosen at this one, so it is done once per node
        // rather than once per sibling. It is the most expensive step in the
        // search and a node has up to thirty six siblings.
        let advanced = self.advance_all(marker, frontier);

        // Children are expanded best first. The bound can only prune against an
        // incumbent, so reaching a strong incumbent early is what decides how
        // much of the tree is ever built.
        let mut children: Vec<(f64, Frontier, Column, Column, RowOrder, RowOrder)> = Vec::new();
        for (col_a, col_b) in &self.pairs[marker] {
            let Some(next_order_a) = order_a.extend(col_a) else {
                // Cut by the homolog symmetry reduction.
                self.stats.pruned += 1;
                continue;
            };
            let Some(next_order_b) = order_b.extend(col_b) else {
                self.stats.pruned += 1;
                continue;
            };
            let Some((next, optimistic)) = self.step(marker, &advanced, col_a, col_b) else {
                // Cut by a bound or by a progeny with no legal state.
                self.stats.pruned += 1;
                continue;
            };
            children.push((optimistic, next, *col_a, *col_b, next_order_a, next_order_b));
        }
        children.sort_by(|l, r| r.0.partial_cmp(&l.0).expect("scores are finite"));
        if marker == 0 {
            if let Some(first) = children.first() { self.stats.root_bound = first.0; }
        }

        for (optimistic, next, col_a, col_b, next_order_a, next_order_b) in children {
            // Re-checked here because the incumbent may have improved while an
            // earlier sibling's subtree was being explored.
            if let Some(best) = &self.best {
                if optimistic < best.total - TIE_TOLERANCE {
                    self.stats.pruned += 1;
                    continue;
                }
            }
            self.stats.expanded += 1;
            self.chosen_a.push(col_a);
            self.chosen_b.push(col_b);
            self.descend(marker + 1, &next, next_order_a, next_order_b);
            self.chosen_a.pop();
            self.chosen_b.pop();
        }
    }

    /// Propagates every progeny's scores from the previous marker to this one.
    ///
    /// At the first marker there is no predecessor, so the carried value is zero
    /// for every state and the arrival score is the marker's own contribution.
    /// Legality is left to the double reduction term, which is negative infinity
    /// where a state is not allowed.
    fn advance_all(&self, marker: usize, frontier: &Frontier) -> Vec<Vec<f64>> {
        if marker == 0 {
            return vec![vec![0.0f64; JOINT]; self.cross.offspring];
        }
        (0..self.cross.offspring)
            .map(|c| {
                let mut out = vec![f64::NEG_INFINITY; JOINT];
                self.terms.advance(marker - 1, &frontier.scores[c], &mut out);
                out
            })
            .collect()
    }

    /// Extends every progeny's arrival scores by one marker.
    ///
    /// Returns `None` when the node cannot beat the incumbent, in which case the
    /// whole subtree below it is skipped. On success it also returns the node's
    /// optimistic total, which orders the siblings.
    fn step(
        &self,
        marker: usize,
        advanced: &[Vec<f64>],
        col_a: &Column,
        col_b: &Column,
    ) -> Option<(Frontier, f64)> {
        let dr = self.terms.dr_at(marker);
        let dose = self.terms.dose_row(col_a, col_b);
        let mut scores = Vec::with_capacity(self.cross.offspring);
        let mut exact = 0.0f64;
        let mut optimistic = 0.0f64;

        for c in 0..self.cross.offspring {
            let prior = &advanced[c];
            let remaining = self.bounds[c].at(marker);
            let gl = &self.cross.child_gl[c][marker];
            let mut arrival = vec![f64::NEG_INFINITY; JOINT];
            let mut best_here = f64::NEG_INFINITY;
            let mut best_reach = f64::NEG_INFINITY;

            for j in 0..JOINT {
                if dr[j] == f64::NEG_INFINITY || prior[j] == f64::NEG_INFINITY {
                    continue;
                }
                // Grouped so the marker's own contribution is summed before it
                // is added to the carried score. Floating point addition is not
                // associative, and this is the order the scorer uses.
                let value = prior[j] + (gl[usize::from(dose[j])] + dr[j]);
                arrival[j] = value;
                if value > best_here {
                    best_here = value;
                }
                let reach = value + remaining[j];
                if reach > best_reach {
                    best_reach = reach;
                }
            }

            if best_here == f64::NEG_INFINITY {
                return None;
            }
            exact += best_here;
            optimistic += best_reach;
            scores.push(arrival);
        }

        // Two admissible bounds, neither dominating the other. The per progeny
        // one frees the columns; the marker one holds them shared and frees the
        // path instead. The smaller is still an upper bound.
        // Two admissible bounds, neither dominating the other. The per progeny
        // one frees the columns; the marker one holds them shared and frees the
        // path instead. The smaller is still an upper bound.
        let mut optimistic = optimistic.min(exact + self.marker_bound.after(marker));

        if let Some(best) = &self.best {
            // Only discard a node that cannot even tie the incumbent. Ties have
            // to stay reachable: when two distinct assignments both attain the
            // optimum, the required answer is the smaller one, and pruning
            // equal-scoring branches would let the order of discovery decide.
            if optimistic < best.total - TIE_TOLERANCE {
                return None;
            }
        }

        // The pair of progeny relaxation is dearer, so it is asked only of
        // nodes the cheaper bounds let through.
        if marker + 1 < self.cross.markers {
            let mut paired = exact;
            let mut single_part = 0.0;
            for (g, &(c1, c2)) in self.pair_bound.groups.iter().enumerate() {
                paired += self.pair_bound.after(g, marker, &scores[c1], &scores[c2]);
                // subtract the exact arrivals counted in `exact` for c1, c2:
                // `after` adds arrival + suffix, so the exact part must be
                // removed once for each of the two progeny.
                let b1 = scores[c1].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let b2 = scores[c2].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                single_part += b1 + b2;
            }
            paired -= single_part;
            // A progeny in no group (odd count) keeps its per progeny term.
            if self.cross.offspring % 2 == 1 {
                let mut in_group = vec![false; self.cross.offspring];
                for &(c1, c2) in &self.pair_bound.groups { in_group[c1] = true; in_group[c2] = true; }
                let c = (0..self.cross.offspring).find(|&c| !in_group[c]).expect("one progeny is unpaired");
                let remaining = self.bounds[c].at(marker);
                let mut best_reach = f64::NEG_INFINITY;
                for j in 0..JOINT {
                    if scores[c][j] == f64::NEG_INFINITY {
                        continue;
                    }
                    let reach = scores[c][j] + remaining[j];
                    if reach > best_reach {
                        best_reach = reach;
                    }
                }
                let b = scores[c].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                paired += best_reach - b;
            }
            optimistic = optimistic.min(paired);
            if let Some(best) = &self.best {
                if optimistic < best.total - TIE_TOLERANCE {
                    return None;
                }
            }
        }

        Some((Frontier { scores, exact }, optimistic))
    }

    fn record(&mut self, frontier: &Frontier) {
        let total = frontier.exact;
        self.stats.leaves += 1;
        if self.stats.first_leaf.is_nan() { self.stats.first_leaf = total; }
        match &self.best {
            Some(best) if total > best.total + TIE_TOLERANCE => {
                self.tied.clear();
                self.tied.push((self.chosen_a.clone(), self.chosen_b.clone()));
            }
            Some(best) if total >= best.total - TIE_TOLERANCE => {
                let entry = (self.chosen_a.clone(), self.chosen_b.clone());
                if !self.tied.contains(&entry) {
                    self.tied.push(entry);
                }
            }
            Some(_) => {}
            None => self.tied.push((self.chosen_a.clone(), self.chosen_b.clone())),
        }
        let replace = match &self.best {
            None => true,
            Some(best) => {
                if total > best.total + TIE_TOLERANCE {
                    true
                } else if total < best.total - TIE_TOLERANCE {
                    false
                } else {
                    self.stats.tie_replacements += 1;
                    // Equal score. The tie is settled by the haplotype matrices,
                    // flattened row major, parent A before parent B, exactly as
                    // the result format specifies. Distinct column assignments
                    // always give distinct matrices, so this decides it.
                    row_major_less(&self.chosen_a, &self.chosen_b, &best.columns_a, &best.columns_b)
                }
            }
        };
        if replace {
            self.best = Some(BestAssignment {
                columns_a: self.chosen_a.clone(),
                columns_b: self.chosen_b.clone(),
                total,
                stats: SearchStats::default(),
            });
        }
    }
}

/// Absolute tolerance separating a genuinely better score from an equal one.
const TIE_TOLERANCE: f64 = 1e-9;

/// Compares two column assignments in the order the result format defines:
/// parent A's four rows flattened row major, then parent B's.
fn row_major_less(
    left_a: &[Column],
    left_b: &[Column],
    right_a: &[Column],
    right_b: &[Column],
) -> bool {
    for (left, right) in [(left_a, right_a), (left_b, right_b)] {
        for h in 0..HOMOLOGS {
            for m in 0..left.len() {
                match left[m][h].cmp(&right[m][h]) {
                    std::cmp::Ordering::Less => return true,
                    std::cmp::Ordering::Greater => return false,
                    std::cmp::Ordering::Equal => {}
                }
            }
        }
    }
    false
}

/// Admissible column pairs for each marker: the product of each parent's
/// admissible columns, parent A's index outermost.
pub fn admissible_pairs(cross: &Cross) -> Vec<Vec<Pair>> {
    use super::columns::columns_for_dosage;
    use crate::cross_model::Parent;
    (0..cross.markers)
        .map(|m| {
            let a = columns_for_dosage(cross.params(Parent::A).dosage[m]);
            let b = columns_for_dosage(cross.params(Parent::B).dosage[m]);
            let mut out = Vec::with_capacity(a.len() * b.len());
            for col_a in &a {
                for col_b in &b {
                    out.push((*col_a, *col_b));
                }
            }
            out
        })
        .collect()
}
