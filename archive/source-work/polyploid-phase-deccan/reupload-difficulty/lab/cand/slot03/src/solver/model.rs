//! Precomputed tables shared by every part of the search.
//!
//! Everything here is a function of the cross alone: the admissible allele
//! columns at each marker, the double reduction and transition log weights, and
//! the dosage a joint inheritance state implies for a pair of columns.

use crate::cross_input::Cross;
use crate::cross_model::{Parent, ALL_STATES};

/// Inheritance states for one parent.
pub const NS: usize = 10;
/// Joint inheritance states, `sA * 10 + sB`.
pub const NJ: usize = 100;

/// Bits of a column mask in the order the canonical haplotype key reads them:
/// homolog 0 is the most significant of the four.
pub fn key_bits(mask: u8) -> u8 {
    ((mask & 1) << 3) | (((mask >> 1) & 1) << 2) | (((mask >> 2) & 1) << 1) | ((mask >> 3) & 1)
}

/// The six unordered pairs of homolog indices, indexed `i * 4 + j` with `i < j`.
pub const PAIRS: [(usize, usize); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];

pub struct Model {
    pub m: usize,
    pub n: usize,
    /// Admissible allele column masks, `[parent][marker]`, ordered by `key_bits`.
    pub cols: [Vec<Vec<u8>>; 2],
    /// `[parent][marker][column][state]` alternate alleles contributed by that
    /// parent's state under that column, `0..=2`.
    pub kcol: [Vec<Vec<[u8; NS]>>; 2],
    /// `[progeny][marker][dosage]` read log likelihoods.
    pub gl: Vec<Vec<[f64; 5]>>,
    /// `[parent][marker][state]` double reduction log weight; `-inf` when illegal.
    pub dr: [Vec<[f64; NS]>; 2],
    /// `[parent][interval][from][to]` transition log weight.
    pub trans: [Vec<[[f64; NS]; NS]>; 2],
    /// `[parent][interval][from][to]`, transposed for backward recursions.
    pub trans_t: [Vec<[[f64; NS]; NS]>; 2],
    /// `[marker][joint state]` sum of both parents' double reduction weights.
    pub drj: Vec<[f64; NJ]>,
    /// `[marker]` admissible `(column_a, column_b)` index pairs, in canonical key order.
    pub pairs: Vec<Vec<(usize, usize)>>,
    /// `[marker][pair][joint state]` progeny dosage.
    pub dose: Vec<Vec<[u8; NJ]>>,
}

impl Model {
    pub fn build(cross: &Cross) -> Model {
        let m = cross.markers;
        let n = cross.offspring;

        let mut cols: [Vec<Vec<u8>>; 2] = [Vec::new(), Vec::new()];
        let mut kcol: [Vec<Vec<[u8; NS]>>; 2] = [Vec::new(), Vec::new()];
        let mut dr: [Vec<[f64; NS]>; 2] = [Vec::new(), Vec::new()];
        let mut trans: [Vec<[[f64; NS]; NS]>; 2] = [Vec::new(), Vec::new()];
        let mut trans_t: [Vec<[[f64; NS]; NS]>; 2] = [Vec::new(), Vec::new()];

        for (p, parent) in Parent::BOTH.iter().enumerate() {
            let params = cross.params(*parent);
            let mut per_marker_cols = Vec::with_capacity(m);
            let mut per_marker_k = Vec::with_capacity(m);
            for mm in 0..m {
                let want = params.dosage[mm];
                let mut list: Vec<u8> =
                    (0u8..16).filter(|mask| mask.count_ones() as u8 == want).collect();
                list.sort_by_key(|mask| key_bits(*mask));
                let mut ks = Vec::with_capacity(list.len());
                for mask in &list {
                    let mut row = [0u8; NS];
                    for (s, state) in ALL_STATES.iter().enumerate() {
                        let a = usize::from(state.low());
                        let b = usize::from(state.high());
                        row[s] = u8::from((mask >> a) & 1) + u8::from((mask >> b) & 1);
                    }
                    ks.push(row);
                }
                per_marker_cols.push(list);
                per_marker_k.push(ks);
            }

            let mut per_marker_dr = Vec::with_capacity(m);
            for mm in 0..m {
                let mut row = [0.0f64; NS];
                for (s, state) in ALL_STATES.iter().enumerate() {
                    row[s] = if state.is_double_reduction() {
                        if params.dr_allowed[mm] {
                            params.dr_rate[mm].ln()
                        } else {
                            f64::NEG_INFINITY
                        }
                    } else {
                        (1.0 - params.dr_rate[mm]).ln()
                    };
                }
                per_marker_dr.push(row);
            }

            let mut per_interval = Vec::with_capacity(m.saturating_sub(1));
            let mut per_interval_t = Vec::with_capacity(m.saturating_sub(1));
            for mm in 0..m.saturating_sub(1) {
                let r = params.recombination[mm];
                let lr = r.ln();
                let l1 = (1.0 - r).ln();
                let mut block = [[0.0f64; NS]; NS];
                let mut block_t = [[0.0f64; NS]; NS];
                for (from, f) in ALL_STATES.iter().enumerate() {
                    for (to, t) in ALL_STATES.iter().enumerate() {
                        let k = f64::from(f.switch_count(*t));
                        let v = k * lr + (2.0 - k) * l1;
                        block[from][to] = v;
                        block_t[to][from] = v;
                    }
                }
                per_interval.push(block);
                per_interval_t.push(block_t);
            }

            cols[p] = per_marker_cols;
            kcol[p] = per_marker_k;
            dr[p] = per_marker_dr;
            trans[p] = per_interval;
            trans_t[p] = per_interval_t;
        }

        let mut drj = Vec::with_capacity(m);
        for mm in 0..m {
            let mut row = [0.0f64; NJ];
            for sa in 0..NS {
                for sb in 0..NS {
                    row[sa * NS + sb] = dr[0][mm][sa] + dr[1][mm][sb];
                }
            }
            drj.push(row);
        }

        let mut pairs = Vec::with_capacity(m);
        let mut dose = Vec::with_capacity(m);
        for mm in 0..m {
            let mut list = Vec::new();
            for ia in 0..cols[0][mm].len() {
                for ib in 0..cols[1][mm].len() {
                    list.push((ia, ib));
                }
            }
            let mut per_pair = Vec::with_capacity(list.len());
            for &(ia, ib) in &list {
                let mut row = [0u8; NJ];
                for sa in 0..NS {
                    for sb in 0..NS {
                        row[sa * NS + sb] = kcol[0][mm][ia][sa] + kcol[1][mm][ib][sb];
                    }
                }
                per_pair.push(row);
            }
            pairs.push(list);
            dose.push(per_pair);
        }

        let gl = cross.child_gl.clone();

        Model { m, n, cols, kcol, gl, dr, trans, trans_t, drj, pairs, dose }
    }

    /// Emission log likelihood for one progeny, marker and joint state under a
    /// column pair index.
    #[inline]
    pub fn emission(&self, progeny: usize, marker: usize, pair: usize, joint: usize) -> f64 {
        self.gl[progeny][marker][usize::from(self.dose[marker][pair][joint])]
    }
}
