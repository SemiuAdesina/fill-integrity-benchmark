
//! Admissible upper bounds used to prune the search.
//!
//! Both bounds answer the same question — given what has been decided up to
//! some marker, what is the most the rest of the cross could still be worth —
//! and both answer it by relaxing a constraint the real problem imposes.
//! Relaxing a constraint can only raise a maximum, which is the entire argument
//! for why pruning against either one is safe.
//!
//! They relax opposite constraints, and neither dominates the other:
//!
//! - [`RemainingBound`] keeps each progeny's path continuous and frees the
//!   parental columns, letting every progeny choose its own most convenient
//!   columns at every future marker.
//! - [`MarkerBound`] keeps one shared column pair per marker, as the real
//!   problem does, and frees the paths by charging every interval its most
//!   favourable transition.
//!
//! The search takes the smaller of the two. That is still an upper bound,
//! because both are, and it is what brings the exact search into reach.

use crate::cross_input::Cross;
use crate::cross_model::Parent;

use super::columns::Column;
use super::progeny_dp::{emission, StaticTerms, JOINT};

/// Admissible remaining score for one progeny, per marker and joint state.
///
/// `at(m)[j]` is an upper bound on what this progeny can still earn over
/// markers `m + 1` onwards, having been in joint state `j` at marker `m`.
pub struct RemainingBound {
    bound: Vec<Vec<f64>>,
}

impl RemainingBound {
    pub fn build(
        cross: &Cross,
        terms: &StaticTerms,
        progeny: usize,
        columns_a: &[Vec<Column>],
        columns_b: &[Vec<Column>],
    ) -> RemainingBound {
        let markers = cross.markers;

        // Best emission available at each marker and joint state, over every
        // admissible column pair there. This is where the columns are freed.
        let mut node_max = Vec::with_capacity(markers);
        for m in 0..markers {
            let dr = terms.dr_at(m);
            let mut row = vec![f64::NEG_INFINITY; JOINT];
            for j in 0..JOINT {
                if dr[j] == f64::NEG_INFINITY {
                    continue;
                }
                let mut best = f64::NEG_INFINITY;
                for col_a in &columns_a[m] {
                    for col_b in &columns_b[m] {
                        let value = emission(cross, progeny, m, j, col_a, col_b);
                        if value > best {
                            best = value;
                        }
                    }
                }
                row[j] = best + dr[j];
            }
            node_max.push(row);
        }

        let mut bound = vec![vec![0.0f64; JOINT]; markers];
        for m in (0..markers.saturating_sub(1)).rev() {
            let mut future = vec![f64::NEG_INFINITY; JOINT];
            for j in 0..JOINT {
                future[j] = node_max[m + 1][j] + bound[m + 1][j];
            }
            // Same factorised sweep as the forward pass, run the other way, so
            // the backward bound costs two ten by ten sweeps per state as well.
            let mut carried = vec![f64::NEG_INFINITY; JOINT];
            terms.advance_backward(m, &future, &mut carried);
            bound[m] = carried;
        }

        RemainingBound { bound }
    }

    pub fn at(&self, marker: usize) -> &[f64] {
        &self.bound[marker]
    }
}

/// Upper bound on the whole cross's remaining score, with columns kept shared.
///
/// [`RemainingBound`] frees the columns per progeny, which with several progeny
/// overshoots by tens of nats and prunes little. This bound holds the columns
/// shared and frees the paths instead.
pub struct MarkerBound {
    suffix: Vec<f64>,
}

impl MarkerBound {
    pub fn build(
        cross: &Cross,
        terms: &StaticTerms,
        columns_a: &[Vec<Column>],
        columns_b: &[Vec<Column>],
    ) -> MarkerBound {
        let markers = cross.markers;

        // The best any single marker can be worth once its column pair is fixed
        // and every progeny then picks its own best joint state there.
        let mut best_node = vec![f64::NEG_INFINITY; markers];
        for m in 0..markers {
            let dr = terms.dr_at(m);
            for col_a in &columns_a[m] {
                for col_b in &columns_b[m] {
                    let mut total = 0.0;
                    for c in 0..cross.offspring {
                        let mut best = f64::NEG_INFINITY;
                        for j in 0..JOINT {
                            if dr[j] == f64::NEG_INFINITY {
                                continue;
                            }
                            let value = emission(cross, c, m, j, col_a, col_b) + dr[j];
                            if value > best {
                                best = value;
                            }
                        }
                        total += best;
                    }
                    if total > best_node[m] {
                        best_node[m] = total;
                    }
                }
            }
        }

        // The largest transition term for one progeny across one interval is the
        // one with no switch, because `r <= 0.5` forces `ln(r) <= ln(1 - r)`.
        let n = cross.offspring as f64;
        let max_transition = |m: usize| -> f64 {
            let ra = cross.params(Parent::A).recombination[m];
            let rb = cross.params(Parent::B).recombination[m];
            2.0 * n * ((1.0 - ra).ln() + (1.0 - rb).ln())
        };

        let mut suffix = vec![0.0f64; markers];
        for m in (0..markers.saturating_sub(1)).rev() {
            suffix[m] = best_node[m + 1] + max_transition(m) + suffix[m + 1];
        }
        MarkerBound { suffix }
    }

    /// Upper bound on everything still to come once `marker` has been decided.
    pub fn after(&self, marker: usize) -> f64 {
        self.suffix[marker] - 0.75
    }
}
