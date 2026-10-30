
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
    /// Progeny dosage for every packed column pair and joint state,
    /// `[mask_a * 16 + mask_b][joint]`. Marker independent.
    dose: Vec<[u8; JOINT]>,
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

        let mut dose = vec![[0u8; JOINT]; 256];
        for ma in 0..16u8 {
            for mb in 0..16u8 {
                let col_a = [ma & 1, (ma >> 1) & 1, (ma >> 2) & 1, (ma >> 3) & 1];
                let col_b = [mb & 1, (mb >> 1) & 1, (mb >> 2) & 1, (mb >> 3) & 1];
                for j in 0..JOINT {
                    let (sa, sb) = split_joint(j);
                    dose[usize::from(ma) * 16 + usize::from(mb)][j] = half_dosage(sa, &col_a) + half_dosage(sb, &col_b);
                }
            }
        }
        StaticTerms {
            dose,
            dr,
            trans: [build_trans(Parent::A), build_trans(Parent::B)],
        }
    }

    /// Dosage of every joint state under a column pair, by packed masks.
    pub fn dose_row(&self, col_a: &Column, col_b: &Column) -> &[u8; JOINT] {
        &self.dose[usize::from(pack(col_a)) * 16 + usize::from(pack(col_b))]
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

/// Packs a column into its four bit mask, homolog `h` at bit `h`.
pub fn pack(column: &Column) -> u8 {
    column[0] | (column[1] << 1) | (column[2] << 2) | (column[3] << 3)
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
