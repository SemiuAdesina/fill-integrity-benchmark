//! Precomputed tables shared by every part of the search.
//!
//! The search never touches `Cross` directly. Everything it needs — admissible
//! allele columns, the dosage each inheritance state implies, legality, the
//! double reduction and transition terms — is flattened here once, into
//! indices the inner loops can use without bound checks or re-derivation.
//!
//! Two indexings are used throughout:
//!
//! * a parent inheritance state index `0..10`, in the canonical order of
//!   `spec/scoring-model.md`;
//! * a joint state index `0..100`, `sa * 10 + sb`, for the pair of states a
//!   progeny carries at one marker.

use crate::cross_input::Cross;
use crate::cross_model::{Parent, ALL_STATES};

/// Inheritance states available to one parent at one marker.
pub const NS: usize = 10;
/// Joint states of one progeny at one marker: `sa * NS + sb`.
pub const NJ: usize = 100;

/// The two homologs a state index selects.
pub fn state_pair(i: usize) -> (usize, usize) {
    (ALL_STATES[i].low() as usize, ALL_STATES[i].high() as usize)
}

/// Every allele column with exactly `dosage` alternate alleles, ascending in
/// bit order so that column indices are ordered independently of the search.
fn admissible_columns(dosage: u8) -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for bits in 0u8..16 {
        if bits.count_ones() as u8 == dosage {
            out.push([bits & 1, (bits >> 1) & 1, (bits >> 2) & 1, (bits >> 3) & 1]);
        }
    }
    out
}

pub struct Model {
    pub markers: usize,
    pub offspring: usize,
    /// Admissible allele columns at each marker, ascending, per parent.
    pub cols_a: Vec<Vec<[u8; 4]>>,
    pub cols_b: Vec<Vec<[u8; 4]>>,
    pub na: Vec<usize>,
    pub nb: Vec<usize>,
    /// `[marker][pair][joint]` progeny dosage, where `pair = ia * nb + ib`.
    pub dose: Vec<Vec<[u8; NJ]>>,
    /// `[marker][joint]` bitmask of dosages reachable by some admissible pair.
    pub dmask: Vec<[u8; NJ]>,
    /// `[marker][joint]` whether the joint state is legal there.
    pub legal: Vec<[bool; NJ]>,
    /// `[marker][joint]` the two double reduction terms, summed.
    pub drj: Vec<[f64; NJ]>,
    /// `[interval][from][to]` transition term for parent A.
    pub ta: Vec<[[f64; NS]; NS]>,
    /// `[interval][from][to]` transition term for parent B.
    pub tb: Vec<[[f64; NS]; NS]>,
    /// `[progeny][marker][dosage]` emission log likelihoods.
    pub gl: Vec<Vec<[f64; 5]>>,
}

impl Model {
    pub fn new(cross: &Cross) -> Model {
        let markers = cross.markers;
        let offspring = cross.offspring;

        let mut cols_a = Vec::with_capacity(markers);
        let mut cols_b = Vec::with_capacity(markers);
        let mut legal = Vec::with_capacity(markers);
        let mut drj = Vec::with_capacity(markers);
        for m in 0..markers {
            cols_a.push(admissible_columns(cross.params(Parent::A).dosage[m]));
            cols_b.push(admissible_columns(cross.params(Parent::B).dosage[m]));

            let allowed_a = cross.params(Parent::A).dr_allowed[m];
            let allowed_b = cross.params(Parent::B).dr_allowed[m];
            let mut l = [true; NJ];
            for sa in 0..NS {
                if ALL_STATES[sa].is_double_reduction() && !allowed_a {
                    for sb in 0..NS {
                        l[sa * NS + sb] = false;
                    }
                }
            }
            for sb in 0..NS {
                if ALL_STATES[sb].is_double_reduction() && !allowed_b {
                    for sa in 0..NS {
                        l[sa * NS + sb] = false;
                    }
                }
            }
            legal.push(l);

            let rate_a = cross.params(Parent::A).dr_rate[m];
            let rate_b = cross.params(Parent::B).dr_rate[m];
            let mut d = [0.0f64; NJ];
            for sa in 0..NS {
                let va = if ALL_STATES[sa].is_double_reduction() {
                    rate_a.ln()
                } else {
                    (1.0 - rate_a).ln()
                };
                for sb in 0..NS {
                    let vb = if ALL_STATES[sb].is_double_reduction() {
                        rate_b.ln()
                    } else {
                        (1.0 - rate_b).ln()
                    };
                    d[sa * NS + sb] = va + vb;
                }
            }
            drj.push(d);
        }

        let mut ta = Vec::with_capacity(markers.saturating_sub(1));
        let mut tb = Vec::with_capacity(markers.saturating_sub(1));
        for m in 0..markers.saturating_sub(1) {
            let ra = cross.params(Parent::A).recombination[m];
            let rb = cross.params(Parent::B).recombination[m];
            let mut a = [[0.0f64; NS]; NS];
            let mut b = [[0.0f64; NS]; NS];
            for s in 0..NS {
                for t in 0..NS {
                    let k = f64::from(ALL_STATES[s].switch_count(ALL_STATES[t]));
                    a[s][t] = k * ra.ln() + (2.0 - k) * (1.0 - ra).ln();
                    b[s][t] = k * rb.ln() + (2.0 - k) * (1.0 - rb).ln();
                }
            }
            ta.push(a);
            tb.push(b);
        }

        let mut dose = Vec::with_capacity(markers);
        let mut dmask = Vec::with_capacity(markers);
        let mut na = Vec::with_capacity(markers);
        let mut nb = Vec::with_capacity(markers);
        for m in 0..markers {
            let mut tables = Vec::with_capacity(cols_a[m].len() * cols_b[m].len());
            for ca in &cols_a[m] {
                for cb in &cols_b[m] {
                    let mut t = [0u8; NJ];
                    for sa in 0..NS {
                        let (x, y) = state_pair(sa);
                        let va = ca[x] + ca[y];
                        for sb in 0..NS {
                            let (u, v) = state_pair(sb);
                            t[sa * NS + sb] = va + cb[u] + cb[v];
                        }
                    }
                    tables.push(t);
                }
            }
            let mut mask = [0u8; NJ];
            for t in &tables {
                for j in 0..NJ {
                    mask[j] |= 1u8 << t[j];
                }
            }
            dose.push(tables);
            dmask.push(mask);
            na.push(cols_a[m].len());
            nb.push(cols_b[m].len());
        }

        Model {
            markers,
            offspring,
            cols_a,
            cols_b,
            na,
            nb,
            dose,
            dmask,
            legal,
            drj,
            ta,
            tb,
            gl: cross.child_gl.clone(),
        }
    }

    /// Flat index of the column pair `(ia, ib)` at `marker`.
    pub fn pair(&self, marker: usize, ia: usize, ib: usize) -> usize {
        ia * self.nb[marker] + ib
    }

    /// Node score of `progeny` at `marker` for every joint state, given the
    /// dosage table of the chosen column pair: emission plus both double
    /// reduction terms. States illegal at that marker stay at `-inf`.
    pub fn node_row(
        &self,
        progeny: usize,
        marker: usize,
        dose: &[u8; NJ],
        out: &mut [f64; NJ],
    ) {
        let g = &self.gl[progeny][marker];
        let dr = &self.drj[marker];
        let legal = &self.legal[marker];
        for j in 0..NJ {
            out[j] = if legal[j] { g[dose[j] as usize] + dr[j] } else { f64::NEG_INFINITY };
        }
    }

    /// Per marker node rows for one progeny under a fixed column assignment,
    /// written into `out`.
    pub fn nodes_for(
        &self,
        progeny: usize,
        assign: &[(usize, usize)],
        out: &mut Vec<[f64; NJ]>,
    ) {
        out.clear();
        for m in 0..self.markers {
            let (ia, ib) = assign[m];
            let dose = &self.dose[m][self.pair(m, ia, ib)];
            let mut row = [f64::NEG_INFINITY; NJ];
            self.node_row(progeny, m, dose, &mut row);
            out.push(row);
        }
    }
}
