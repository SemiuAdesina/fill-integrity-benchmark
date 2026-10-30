
//! Per progeny dynamic programming over the hundred joint inheritance states.
//!
//! Two facts about the objective shape the whole search and are worth stating
//! plainly, because missing either one is what makes a direct attack look
//! hopeless.
//!
//! The transition and double reduction terms do not depend on the parental
//! alleles at all. Only the emission term does, and it depends on them only
//! through the progeny's dosage. So propagating a progeny's scores from one
//! marker to the next can be done once, before any allele column is chosen,
//! and reused for every column the search tries at that marker.
//!
//! The transition itself factorises across parents. Maximising over a hundred
//! predecessors for each of a hundred successors looks like ten thousand
//! comparisons; done as two successive one parent maximisations it is two
//! thousand. Both facts together are what brings the exact search into reach.

use crate::cross_input::Cross;
use crate::cross_model::{Parent, State, ALL_STATES, STATES};
use crate::scoring::{double_reduction_term, transition_term};

use super::columns::Column;

/// Number of joint states: ten for each parent.
pub const JOINT: usize = STATES * STATES;

/// Joint state index from the two per parent canonical indices.
pub fn joint_index(a: usize, b: usize) -> usize {
    a * STATES + b
}

/// The two per parent states of a joint index.
pub fn split_joint(j: usize) -> (State, State) {
    (ALL_STATES[j / STATES], ALL_STATES[j % STATES])
}

/// Alternate alleles a progeny receives from one parent, given that parent's
/// column at the marker.
///
/// A double reduction state names the same homolog twice and so contributes
/// that homolog's allele twice.
pub fn half_dosage(state: State, column: &Column) -> u8 {
    column[state.low() as usize] + column[state.high() as usize]
}

/// Terms of the objective that do not depend on the parental alleles.
///
/// Built once per cross and shared by every progeny.
pub struct StaticTerms {
    /// Double reduction contribution, `[marker][joint]`. Negative infinity
    /// where either parent's state is an illegal double reduction.
    dr: Vec<Vec<f64>>,
    /// Per parent transition matrices, `[parent][marker][from][to]`.
    trans: [Vec<Vec<f64>>; 2],
}

impl StaticTerms {
    pub fn build(cross: &Cross) -> StaticTerms {
        let markers = cross.markers;

        let mut dr = Vec::with_capacity(markers);
        for m in 0..markers {
            let mut row = vec![0.0f64; JOINT];
            for j in 0..JOINT {
                let (sa, sb) = split_joint(j);
                let legal_a = !sa.is_double_reduction() || cross.params(Parent::A).dr_allowed[m];
                let legal_b = !sb.is_double_reduction() || cross.params(Parent::B).dr_allowed[m];
                row[j] = if legal_a && legal_b {
                    double_reduction_term(cross, Parent::A, sa, m)
                        + double_reduction_term(cross, Parent::B, sb, m)
                } else {
                    f64::NEG_INFINITY
                };
            }
            dr.push(row);
        }

        let build_trans = |parent: Parent| -> Vec<Vec<f64>> {
            (0..markers.saturating_sub(1))
                .map(|m| {
                    let mut mat = vec![0.0f64; STATES * STATES];
                    for (i, from) in ALL_STATES.iter().enumerate() {
                        for (k, to) in ALL_STATES.iter().enumerate() {
                            mat[i * STATES + k] = transition_term(cross, parent, *from, *to, m);
                        }
                    }
                    mat
                })
                .collect()
        };

        StaticTerms {
            dr,
            trans: [build_trans(Parent::A), build_trans(Parent::B)],
        }
    }

    /// Double reduction contribution for every joint state at one marker.
    pub fn dr_at(&self, marker: usize) -> &[f64] {
        &self.dr[marker]
    }

    /// Propagates arrival scores from `marker` to `marker + 1`.
    ///
    /// Runs the parent `A` maximisation first, then parent `B`, which is what
    /// turns the hundred by hundred maximisation into two ten by ten sweeps per
    /// state.
    pub fn advance(&self, marker: usize, scores: &[f64], out: &mut [f64]) {
        let ta = &self.trans[0][marker];
        let tb = &self.trans[1][marker];

        // Stage one: choose parent A's next state, keeping parent B's current.
        let mut staged = [f64::NEG_INFINITY; JOINT];
        for ib in 0..STATES {
            for next_a in 0..STATES {
                let mut best = f64::NEG_INFINITY;
                for ia in 0..STATES {
                    let here = scores[joint_index(ia, ib)];
                    if here == f64::NEG_INFINITY {
                        continue;
                    }
                    let candidate = here + ta[ia * STATES + next_a];
                    if candidate > best {
                        best = candidate;
                    }
                }
                staged[joint_index(next_a, ib)] = best;
            }
        }

        // Stage two: choose parent B's next state.
        for next_a in 0..STATES {
            for next_b in 0..STATES {
                let mut best = f64::NEG_INFINITY;
                for ib in 0..STATES {
                    let here = staged[joint_index(next_a, ib)];
                    if here == f64::NEG_INFINITY {
                        continue;
                    }
                    let candidate = here + tb[ib * STATES + next_b];
                    if candidate > best {
                        best = candidate;
                    }
                }
                out[joint_index(next_a, next_b)] = best;
            }
        }
    }

    /// As `advance`, and also records for each arrival state the joint state it
    /// came from, so a path can be traced back through the factorised sweep.
    pub fn advance_traced(&self, marker: usize, scores: &[f64], out: &mut [f64], back: &mut [u8]) {
        let ta = &self.trans[0][marker];
        let tb = &self.trans[1][marker];
        let mut staged = [f64::NEG_INFINITY; JOINT];
        let mut staged_from = [0u8; JOINT];
        for ib in 0..STATES {
            for next_a in 0..STATES {
                let mut best = f64::NEG_INFINITY;
                let mut arg = 0u8;
                for ia in 0..STATES {
                    let here = scores[joint_index(ia, ib)];
                    if here == f64::NEG_INFINITY {
                        continue;
                    }
                    let candidate = here + ta[ia * STATES + next_a];
                    if candidate > best {
                        best = candidate;
                        arg = ia as u8;
                    }
                }
                staged[joint_index(next_a, ib)] = best;
                staged_from[joint_index(next_a, ib)] = arg;
            }
        }
        for next_a in 0..STATES {
            for next_b in 0..STATES {
                let mut best = f64::NEG_INFINITY;
                let mut arg_ib = 0usize;
                for ib in 0..STATES {
                    let here = staged[joint_index(next_a, ib)];
                    if here == f64::NEG_INFINITY {
                        continue;
                    }
                    let candidate = here + tb[ib * STATES + next_b];
                    if candidate > best {
                        best = candidate;
                        arg_ib = ib;
                    }
                }
                out[joint_index(next_a, next_b)] = best;
                let ia = staged_from[joint_index(next_a, arg_ib)] as usize;
                back[joint_index(next_a, next_b)] = joint_index(ia, arg_ib) as u8;
            }
        }
    }

    /// Propagates values at `marker + 1` back to `marker`.
    ///
    /// Where `advance` asks "what is the best way to arrive here", this asks
    /// "what is the best I can still do from here". Same factorisation, run in
    /// the other direction, so it is also two ten by ten sweeps per state.
    pub fn advance_backward(&self, marker: usize, future: &[f64], out: &mut [f64]) {
        let ta = &self.trans[0][marker];
        let tb = &self.trans[1][marker];

        // Stage one: eliminate parent B's successor.
        let mut staged = [f64::NEG_INFINITY; JOINT];
        for next_a in 0..STATES {
            for ib in 0..STATES {
                let mut best = f64::NEG_INFINITY;
                for next_b in 0..STATES {
                    let there = future[joint_index(next_a, next_b)];
                    if there == f64::NEG_INFINITY {
                        continue;
                    }
                    let candidate = there + tb[ib * STATES + next_b];
                    if candidate > best {
                        best = candidate;
                    }
                }
                staged[joint_index(next_a, ib)] = best;
            }
        }

        // Stage two: eliminate parent A's successor.
        for ia in 0..STATES {
            for ib in 0..STATES {
                let mut best = f64::NEG_INFINITY;
                for next_a in 0..STATES {
                    let there = staged[joint_index(next_a, ib)];
                    if there == f64::NEG_INFINITY {
                        continue;
                    }
                    let candidate = there + ta[ia * STATES + next_a];
                    if candidate > best {
                        best = candidate;
                    }
                }
                out[joint_index(ia, ib)] = best;
            }
        }
    }
}

/// Emission contribution for one progeny at one marker under a column pair.
pub fn emission(
    cross: &Cross,
    progeny: usize,
    marker: usize,
    joint: usize,
    col_a: &Column,
    col_b: &Column,
) -> f64 {
    let (sa, sb) = split_joint(joint);
    let dosage = half_dosage(sa, col_a) + half_dosage(sb, col_b);
    cross.child_gl[progeny][marker][usize::from(dosage)]
}
