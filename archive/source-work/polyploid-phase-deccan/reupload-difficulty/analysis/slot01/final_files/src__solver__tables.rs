
//! Per-marker tables shared by every part of the search.
//!
//! Everything here is a pure function of the cross: which allele columns a
//! marker admits, what a state is worth under double reduction and
//! recombination, and how large the emission at a joint state can possibly be.
//! Precomputing them keeps the inner loops of the search free of branching on
//! the input format.

use crate::cross_input::Cross;
use crate::cross_model::{Parent, ALL_STATES, STATES};

/// Joint `(parent A state, parent B state)` combinations a progeny can hold.
pub const JOINTS: usize = STATES * STATES;

/// Bit `u` of the sets in [`Tables::achievable`], for `u` in `0..=2`.
pub const fn u_bit(u: u8) -> u8 {
    1u8 << u
}

/// Everything the search needs about a cross, in flat arrays.
pub struct Tables {
    pub markers: usize,
    pub offspring: usize,

    /// Admissible allele columns, `[parent][marker]`, each a four bit mask of
    /// the homologs carrying the alternate allele.
    pub cols: [Vec<Vec<u8>>; 2],
    /// `[parent][marker][state]` double reduction log term; `-inf` where the
    /// state is illegal.
    pub dr: [Vec<[f64; STATES]>; 2],
    /// `[parent][marker][state]` whether the state may be used at all.
    pub legal: [Vec<[bool; STATES]>; 2],
    /// `[parent][interval][k]` transition log term for `k` changed homologs.
    pub trans: [Vec<[f64; 3]>; 2],
    /// `[parent][interval][from * STATES + to]` transition log term.
    pub tmat: [Vec<[f64; STATES * STATES]>; 2],
    /// `[from][to]` number of inherited homologs that changed.
    pub switch: [[u8; STATES]; STATES],
    /// `[mask][state]` how many alternate alleles the state picks up.
    pub u_of: [[u8; STATES]; 16],
    /// `[parent][marker][state]` bit set of `u` values some admissible column
    /// realises for that state.
    pub achievable: [Vec<[u8; STATES]>; 2],

    /// `[marker][joint]` whether the joint state is legal.
    pub legj: Vec<[bool; JOINTS]>,
    /// `[marker][joint]` combined double reduction term of both parents.
    pub drj: Vec<[f64; JOINTS]>,

    /// `[progeny][marker][joint]` largest emission any admissible column pair
    /// can reach at that joint state.
    pub ub: Vec<Vec<[f64; JOINTS]>>,
}

impl Tables {
    pub fn build(cross: &Cross) -> Tables {
        let markers = cross.markers;
        let offspring = cross.offspring;

        let mut cols: [Vec<Vec<u8>>; 2] = [Vec::new(), Vec::new()];
        let mut dr: [Vec<[f64; STATES]>; 2] = [Vec::new(), Vec::new()];
        let mut legal: [Vec<[bool; STATES]>; 2] = [Vec::new(), Vec::new()];
        let mut achievable: [Vec<[u8; STATES]>; 2] = [Vec::new(), Vec::new()];
        let mut trans: [Vec<[f64; 3]>; 2] = [Vec::new(), Vec::new()];
        let mut tmat: [Vec<[f64; STATES * STATES]>; 2] = [Vec::new(), Vec::new()];

        for parent in Parent::BOTH {
            let p = parent.index();
            let params = cross.params(parent);
            let mut pcols = Vec::with_capacity(markers);
            let mut pdr = Vec::with_capacity(markers);
            let mut plegal = Vec::with_capacity(markers);
            let mut pach = Vec::with_capacity(markers);

            for m in 0..markers {
                let dose = params.dosage[m];
                let mut list = Vec::new();
                for mask in 0u8..16 {
                    if mask.count_ones() as u8 == dose {
                        list.push(mask);
                    }
                }
                pcols.push(list);

                let rate = params.dr_rate[m];
                let allowed = params.dr_allowed[m];
                let mut row = [0.0f64; STATES];
                let mut ok = [false; STATES];
                for (s, state) in ALL_STATES.iter().enumerate() {
                    let is_dr = state.is_double_reduction();
                    let usable = !is_dr || allowed;
                    ok[s] = usable;
                    row[s] = if usable {
                        if is_dr { rate.ln() } else { (1.0 - rate).ln() }
                    } else {
                        f64::NEG_INFINITY
                    };
                }
                pdr.push(row);
                plegal.push(ok);

                let mut ach = [0u8; STATES];
                for (s, _state) in ALL_STATES.iter().enumerate() {
                    let mut bits = 0u8;
                    for mask in &pcols[m] {
                        bits |= u_bit(u_of(*mask, s));
                    }
                    ach[s] = bits;
                }
                pach.push(ach);
            }
            cols[p] = pcols;
            dr[p] = pdr;
            legal[p] = plegal;
            achievable[p] = pach;

            let rec = &params.recombination;
            let mut values = Vec::with_capacity(markers.saturating_sub(1));
            let mut mats = Vec::with_capacity(markers.saturating_sub(1));
            for r in rec.iter() {
                let ln_r = r.ln();
                let ln_c = (1.0 - r).ln();
                let mut row = [0.0f64; 3];
                for k in 0..3u32 {
                    row[k as usize] = f64::from(k) * ln_r + f64::from(2 - k) * ln_c;
                }
                let mut mat = [0.0f64; STATES * STATES];
                for (i, from) in ALL_STATES.iter().enumerate() {
                    for (j, to) in ALL_STATES.iter().enumerate() {
                        mat[i * STATES + j] = row[from.switch_count(*to) as usize];
                    }
                }
                values.push(row);
                mats.push(mat);
            }
            trans[p] = values;
            tmat[p] = mats;
        }

        let mut switch = [[0u8; STATES]; STATES];
        for (i, from) in ALL_STATES.iter().enumerate() {
            for (j, to) in ALL_STATES.iter().enumerate() {
                switch[i][j] = from.switch_count(*to) as u8;
            }
        }

        let mut u_table = [[0u8; STATES]; 16];
        for mask in 0..16u8 {
            for (s, _state) in ALL_STATES.iter().enumerate() {
                u_table[mask as usize][s] = u_of(mask, s);
            }
        }

        let mut legj = Vec::with_capacity(markers);
        let mut drj = Vec::with_capacity(markers);
        for m in 0..markers {
            let mut lj = [false; JOINTS];
            let mut dj = [0.0f64; JOINTS];
            for sa in 0..STATES {
                for sb in 0..STATES {
                    let j = sa * STATES + sb;
                    lj[j] = legal[0][m][sa] && legal[1][m][sb];
                    dj[j] = dr[0][m][sa] + dr[1][m][sb];
                }
            }
            legj.push(lj);
            drj.push(dj);
        }

        let mut ub = Vec::with_capacity(offspring);
        for c in 0..offspring {
            let mut rows = Vec::with_capacity(markers);
            for m in 0..markers {
                let mut row = [f64::NEG_INFINITY; JOINTS];
                for sa in 0..STATES {
                    if !legal[0][m][sa] {
                        continue;
                    }
                    let bits_a = achievable[0][m][sa];
                    for sb in 0..STATES {
                        if !legal[1][m][sb] {
                            continue;
                        }
                        let j = sa * STATES + sb;
                        let bits_b = achievable[1][m][sb];
                        let mut best = f64::NEG_INFINITY;
                        for ua in 0..3u8 {
                            if bits_a & u_bit(ua) == 0 {
                                continue;
                            }
                            for uby in 0..3u8 {
                                if bits_b & u_bit(uby) == 0 {
                                    continue;
                                }
                                let v = cross.child_gl[c][m][(ua + uby) as usize];
                                if v > best {
                                    best = v;
                                }
                            }
                        }
                        row[j] = best;
                    }
                }
                rows.push(row);
            }
            ub.push(rows);
        }

        Tables {
            markers,
            offspring,
            cols,
            dr,
            legal,
            trans,
            tmat,
            switch,
            u_of: u_table,
            achievable,
            legj,
            drj,
            ub,
        }
    }

    /// Number of admissible columns of `parent` at `marker`.
    pub fn n_cols(&self, parent: usize, marker: usize) -> usize {
        self.cols[parent][marker].len()
    }
}

/// How many alternate alleles state `s` picks up from the column `mask`.
pub fn u_of(mask: u8, s: usize) -> u8 {
    let state = ALL_STATES[s];
    ((mask >> state.low()) & 1) + ((mask >> state.high()) & 1)
}
