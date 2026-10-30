
//! Precomputed scoring tables.
//!
//! Everything here is a pure function of the cross, computed once. The terms
//! are those of `spec/scoring-model.md`: the emission is left as the raw
//! `child_gl` table because it is the only part that depends on the parental
//! alleles, and the double reduction and transition terms are flattened into
//! arrays so the inner loops of the search never call `ln`.

use crate::cross_input::Cross;
use crate::cross_model::{Parent, ALL_STATES};

/// Number of inheritance states for one parent at one marker.
pub const NS: usize = 10;

/// Low homolog of each canonical state index.
pub const SA: [u8; NS] = [0, 0, 0, 0, 1, 1, 1, 2, 2, 3];
/// High homolog of each canonical state index.
pub const SB: [u8; NS] = [0, 1, 2, 3, 1, 2, 3, 2, 3, 3];

/// Canonical state index of the pair `(a, b)`, which need not be ordered.
pub const STATE_INDEX: [[u8; 4]; 4] = [
    [0, 1, 2, 3],
    [1, 4, 5, 6],
    [2, 5, 7, 8],
    [3, 6, 8, 9],
];

/// Everything the search needs about a cross, in flat arrays.
pub struct Ctx {
    pub markers: usize,
    pub offspring: usize,
    /// `[progeny][marker][dosage]`, the only haplotype dependent term.
    pub gl: Vec<Vec<[f64; 5]>>,
    /// `[parent][interval][from * NS + to]` transition log likelihood.
    pub trans: [Vec<[f64; NS * NS]>; 2],
    /// `[parent][marker][state]` double reduction log likelihood.
    pub dr: [Vec<[f64; NS]>; 2],
    /// `[marker][sa * NS + sb]` the two double reduction terms added.
    pub dr_sum: Vec<[f64; NS * NS]>,
    /// `[parent][marker][state]`, whether the state is legal there.
    pub legal: [Vec<[bool; NS]>; 2],
    /// `[column][state]` alternate alleles contributed by that state.
    pub contrib: [[u8; NS]; 16],
    /// `[parent][marker]` admissible allele columns, ascending.
    pub opts: [Vec<Vec<u8>>; 2],
}

/// All allele columns with exactly `dosage` alternate alleles, ascending.
pub fn columns_with_dosage(dosage: u8) -> Vec<u8> {
    (0u8..16).filter(|c| c.count_ones() as u8 == dosage).collect()
}

impl Ctx {
    pub fn new(cross: &Cross) -> Ctx {
        let markers = cross.markers;
        let offspring = cross.offspring;

        let mut contrib = [[0u8; NS]; 16];
        for column in 0u8..16 {
            for i in 0..NS {
                contrib[usize::from(column)][i] =
                    ((column >> SA[i]) & 1) + ((column >> SB[i]) & 1);
            }
        }

        let mut trans = [Vec::new(), Vec::new()];
        let mut dr = [Vec::new(), Vec::new()];
        let mut legal = [Vec::new(), Vec::new()];
        let mut opts = [Vec::new(), Vec::new()];

        for (p, parent) in [Parent::A, Parent::B].iter().enumerate() {
            let params = cross.params(*parent);

            let mut intervals = Vec::with_capacity(markers.saturating_sub(1));
            for interval in 0..markers.saturating_sub(1) {
                let r = params.recombination[interval];
                let ln_r = r.ln();
                let ln_c = (1.0 - r).ln();
                let mut table = [0.0; NS * NS];
                for from in 0..NS {
                    for to in 0..NS {
                        let k = f64::from(ALL_STATES[from].switch_count(ALL_STATES[to]));
                        table[from * NS + to] = k * ln_r + (2.0 - k) * ln_c;
                    }
                }
                intervals.push(table);
            }
            trans[p] = intervals;

            let mut per_marker = Vec::with_capacity(markers);
            let mut legality = Vec::with_capacity(markers);
            for m in 0..markers {
                let rate = params.dr_rate[m];
                let allowed = params.dr_allowed[m];
                let mut terms = [0.0; NS];
                let mut ok = [true; NS];
                for i in 0..NS {
                    terms[i] = if ALL_STATES[i].is_double_reduction() {
                        rate.ln()
                    } else {
                        (1.0 - rate).ln()
                    };
                    ok[i] = !ALL_STATES[i].is_double_reduction() || allowed;
                }
                per_marker.push(terms);
                legality.push(ok);
            }
            dr[p] = per_marker;
            legal[p] = legality;
            opts[p] = params.dosage.iter().map(|d| columns_with_dosage(*d)).collect();
        }

        let mut dr_sum = Vec::with_capacity(markers);
        for m in 0..markers {
            let mut table = [0.0; NS * NS];
            for i in 0..NS {
                for j in 0..NS {
                    table[i * NS + j] = dr[0][m][i] + dr[1][m][j];
                }
            }
            dr_sum.push(table);
        }

        Ctx {
            markers,
            offspring,
            gl: cross.child_gl.clone(),
            trans,
            dr,
            dr_sum,
            legal,
            contrib,
            opts,
        }
    }

    /// Alternate alleles contributed at `marker` by each state, for one column.
    pub fn contrib_a(&self, column: u8) -> [u8; NS] {
        self.contrib[usize::from(column)]
    }
}
