
//! Precomputed tables and the inner loop of the search.
//!
//! Everything here is mechanical: the per marker admissible allele columns, the
//! transition and double reduction terms, and a Viterbi pass over the hundred
//! joint inheritance states for one progeny against a fixed parental
//! assignment.

use crate::cross_input::Cross;
use crate::cross_model::{Parent, ALL_STATES, HOMOLOGS};

/// States per parent, `C(4,2) + 4`.
pub const NS: usize = 10;
/// Joint states, `NS * NS`.
pub const NJ: usize = 100;

/// The two parents as an index, `A` then `B`.
pub const PA: usize = 0;
pub const PB: usize = 1;

/// Everything the search needs from the cross, precomputed once.
pub struct Tables {
    pub markers: usize,
    pub offspring: usize,
    /// Admissible allele columns per parent and marker, as a bit mask over the
    /// four homologs: bit `h` is set when homolog `h` carries the alternate.
    pub cols: [Vec<Vec<u8>>; 2],
    /// `contrib[p][m][opt][s]`: alternate alleles contributed by state `s` of
    /// parent `p` at marker `m` under column option `opt`. Zero to two.
    pub contrib: [Vec<Vec<[u8; NS]>>; 2],
    /// `trans[p][interval][prev * NS + next]`: transition term for one parent.
    pub trans: [Vec<[f64; NS * NS]>; 2],
    /// `dr[p][m][s]`: double reduction term, `NEG_INFINITY` where illegal.
    pub dr: [Vec<[f64; NS]>; 2],
    /// `gl[progeny][marker][dosage]`.
    pub gl: Vec<Vec<[f64; 5]>>,
    /// `drj[m][joint]`: both parents' double reduction terms, `NEG_INFINITY`
    /// where the joint state is illegal at that marker.
    pub drj: Vec<[f64; NJ]>,
    /// `legalj[m][joint]`.
    pub legalj: Vec<[bool; NJ]>,
}

impl Tables {
    pub fn build(cross: &Cross) -> Tables {
        let markers = cross.markers;
        let mut cols: [Vec<Vec<u8>>; 2] = [Vec::new(), Vec::new()];
        let mut contrib: [Vec<Vec<[u8; NS]>>; 2] = [Vec::new(), Vec::new()];
        let mut trans: [Vec<[f64; NS * NS]>; 2] = [Vec::new(), Vec::new()];
        let mut dr: [Vec<[f64; NS]>; 2] = [Vec::new(), Vec::new()];

        for (p, parent) in [Parent::A, Parent::B].iter().enumerate() {
            let params = cross.params(*parent);
            let mut mark_cols = Vec::with_capacity(markers);
            let mut mark_contrib = Vec::with_capacity(markers);
            for m in 0..markers {
                let dosage = params.dosage[m];
                let mut options: Vec<u8> = Vec::new();
                for mask in 0u8..16 {
                    if mask.count_ones() as u8 == dosage {
                        options.push(mask);
                    }
                }
                let mut per_option: Vec<[u8; NS]> = Vec::with_capacity(options.len());
                for &mask in &options {
                    let mut row = [0u8; NS];
                    for (s, state) in ALL_STATES.iter().enumerate() {
                        let a = usize::from(state.low());
                        let b = usize::from(state.high());
                        row[s] = ((mask >> a) & 1) + ((mask >> b) & 1);
                    }
                    per_option.push(row);
                }
                mark_cols.push(options);
                mark_contrib.push(per_option);
            }
            cols[p] = mark_cols;
            contrib[p] = mark_contrib;

            let mut mark_trans: Vec<[f64; NS * NS]> = Vec::with_capacity(markers.saturating_sub(1));
            for m in 0..markers.saturating_sub(1) {
                let r = params.recombination[m];
                let ln_r = r.ln();
                let ln_1r = (1.0 - r).ln();
                let mut row = [0.0f64; NS * NS];
                for (i, from) in ALL_STATES.iter().enumerate() {
                    for (j, to) in ALL_STATES.iter().enumerate() {
                        let k = f64::from(from.switch_count(*to));
                        row[i * NS + j] = k * ln_r + (2.0 - k) * ln_1r;
                    }
                }
                mark_trans.push(row);
            }
            trans[p] = mark_trans;

            let mut mark_dr: Vec<[f64; NS]> = Vec::with_capacity(markers);
            for m in 0..markers {
                let mut row = [0.0f64; NS];
                for (s, state) in ALL_STATES.iter().enumerate() {
                    row[s] = if state.is_double_reduction() {
                        if params.dr_allowed[m] {
                            params.dr_rate[m].ln()
                        } else {
                            f64::NEG_INFINITY
                        }
                    } else {
                        (1.0 - params.dr_rate[m]).ln()
                    };
                }
                mark_dr.push(row);
            }
            dr[p] = mark_dr;
        }

        let mut drj = Vec::with_capacity(markers);
        let mut legalj = Vec::with_capacity(markers);
        for m in 0..markers {
            let mut row = [0.0f64; NJ];
            let mut legal = [false; NJ];
            for sa in 0..NS {
                for sb in 0..NS {
                    let j = sa * NS + sb;
                    row[j] = dr[PA][m][sa] + dr[PB][m][sb];
                    legal[j] = dr[PA][m][sa].is_finite() && dr[PB][m][sb].is_finite();
                }
            }
            drj.push(row);
            legalj.push(legal);
        }

        Tables {
            markers,
            offspring: cross.offspring,
            cols,
            contrib,
            trans,
            dr,
            gl: cross.child_gl.clone(),
            drj,
            legalj,
        }
    }

    /// Emission of every joint state for one progeny at one marker.
    pub fn emission_row(
        &self,
        progeny: usize,
        marker: usize,
        opt_a: usize,
        opt_b: usize,
        out: &mut [f64; NJ],
    ) {
        let ea = &self.contrib[PA][marker][opt_a];
        let eb = &self.contrib[PB][marker][opt_b];
        let gl = &self.gl[progeny][marker];
        for sa in 0..NS {
            for sb in 0..NS {
                out[sa * NS + sb] = gl[usize::from(ea[sa] + eb[sb])];
            }
        }
    }

    /// Best score for one progeny against a fixed parental assignment.
    ///
    /// `path`, when supplied, receives the joint state index at each marker.
    pub fn viterbi(
        &self,
        progeny: usize,
        xa: &[usize],
        xb: &[usize],
        path: Option<&mut Vec<usize>>,
    ) -> f64 {
        let markers = self.markers;
        let want_path = path.is_some();
        let mut v = [f64::NEG_INFINITY; NJ];
        let mut emit = [0.0f64; NJ];
        self.emission_row(progeny, 0, xa[0], xb[0], &mut emit);
        for j in 0..NJ {
            if self.legalj[0][j] {
                v[j] = emit[j] + self.drj[0][j];
            }
        }

        let mut arg_a: Vec<[u8; NJ]> = Vec::new();
        let mut arg_b: Vec<[u8; NJ]> = Vec::new();
        if want_path {
            arg_a = vec![[0u8; NJ]; markers];
            arg_b = vec![[0u8; NJ]; markers];
        }

        for m in 1..markers {
            let ta = &self.trans[PA][m - 1];
            let tb = &self.trans[PB][m - 1];
            // x[pa * NS + sb] = max over pb of v[pa * NS + pb] + tb[pb * NS + sb]
            let mut x = [f64::NEG_INFINITY; NJ];
            for pa in 0..NS {
                for sb in 0..NS {
                    let mut best = f64::NEG_INFINITY;
                    let mut arg = 0u8;
                    for pb in 0..NS {
                        let val = v[pa * NS + pb] + tb[pb * NS + sb];
                        if val > best {
                            best = val;
                            arg = pb as u8;
                        }
                    }
                    x[pa * NS + sb] = best;
                    if want_path {
                        arg_b[m][pa * NS + sb] = arg;
                    }
                }
            }
            self.emission_row(progeny, m, xa[m], xb[m], &mut emit);
            let mut next = [f64::NEG_INFINITY; NJ];
            for sa in 0..NS {
                for sb in 0..NS {
                    let j = sa * NS + sb;
                    if !self.legalj[m][j] {
                        continue;
                    }
                    let mut best = f64::NEG_INFINITY;
                    let mut arg = 0u8;
                    for pa in 0..NS {
                        let val = x[pa * NS + sb] + ta[pa * NS + sa];
                        if val > best {
                            best = val;
                            arg = pa as u8;
                        }
                    }
                    next[j] = best + emit[j] + self.drj[m][j];
                    if want_path {
                        arg_a[m][j] = arg;
                    }
                }
            }
            v = next;
        }

        let mut best = f64::NEG_INFINITY;
        let mut cursor = 0usize;
        for j in 0..NJ {
            if v[j] > best {
                best = v[j];
                cursor = j;
            }
        }

        if let Some(out) = path {
            out.clear();
            out.resize(markers, 0);
            let mut cur = cursor;
            for m in (0..markers).rev() {
                out[m] = cur;
                if m > 0 {
                    let sb = cur % NS;
                    let pa = usize::from(arg_a[m][cur]);
                    let pb = usize::from(arg_b[m][pa * NS + sb]);
                    cur = pa * NS + pb;
                }
            }
        }
        best
    }

    /// Total over every progeny against a fixed parental assignment.
    pub fn evaluate(&self, xa: &[usize], xb: &[usize]) -> f64 {
        let mut total = 0.0f64;
        for c in 0..self.offspring {
            total += self.viterbi(c, xa, xb, None);
        }
        total
    }

    /// Total and paths for every progeny against a fixed parental assignment.
    pub fn best_paths(&self, xa: &[usize], xb: &[usize]) -> (f64, Vec<Vec<usize>>) {
        let mut total = 0.0f64;
        let mut paths = Vec::with_capacity(self.offspring);
        for c in 0..self.offspring {
            let mut path = Vec::new();
            total += self.viterbi(c, xa, xb, Some(&mut path));
            paths.push(path);
        }
        (total, paths)
    }

    /// Score of an explicit set of joint state paths under the given columns.
    pub fn score_paths(&self, paths: &[Vec<usize>], xa: &[usize], xb: &[usize]) -> f64 {
        let mut total = 0.0f64;
        for (c, path) in paths.iter().enumerate() {
            for m in 0..self.markers {
                let j = path[m];
                let sa = j / NS;
                let sb = j % NS;
                let dosage = self.contrib[PA][m][xa[m]][sa] + self.contrib[PB][m][xb[m]][sb];
                total += self.gl[c][m][usize::from(dosage)];
                total += self.drj[m][j];
                if m + 1 < self.markers {
                    let k = path[m + 1];
                    total += self.trans[PA][m][j / NS * NS + k / NS];
                    total += self.trans[PB][m][j % NS * NS + k % NS];
                }
            }
        }
        total
    }
}

/// Best columns at every marker for an explicit set of joint state paths.
///
/// With the paths fixed the markers decouple, so this is exact and cheap. It
/// returns the columns and the score they give.
pub fn optimize_columns(t: &Tables, paths: &[Vec<usize>]) -> (f64, Vec<usize>, Vec<usize>) {
    let mut xa = vec![0usize; t.markers];
    let mut xb = vec![0usize; t.markers];
    let mut total = 0.0f64;
    for m in 0..t.markers {
        let na = t.contrib[PA][m].len();
        let nb = t.contrib[PB][m].len();
        let mut best = f64::NEG_INFINITY;
        let mut best_pair = (0usize, 0usize);
        let mut emission = 0.0f64;
        for oa in 0..na {
            let ea = &t.contrib[PA][m][oa];
            for ob in 0..nb {
                let eb = &t.contrib[PB][m][ob];
                let mut sum = 0.0f64;
                for (c, path) in paths.iter().enumerate() {
                    let j = path[m];
                    let dosage = ea[j / NS] + eb[j % NS];
                    sum += t.gl[c][m][usize::from(dosage)];
                }
                if sum > best {
                    best = sum;
                    emission = sum;
                    best_pair = (oa, ob);
                }
            }
        }
        xa[m] = best_pair.0;
        xb[m] = best_pair.1;
        total += emission;
        // Transition and double reduction terms do not depend on the columns.
        for path in paths.iter() {
            let j = path[m];
            total += t.drj[m][j];
            if m + 1 < t.markers {
                let k = path[m + 1];
                total += t.trans[PA][m][j / NS * NS + k / NS];
                total += t.trans[PB][m][j % NS * NS + k % NS];
            }
        }
    }
    (total, xa, xb)
}

/// The four homologs as an index into `ALL_STATES`-style pairs.
pub const fn pair_index(a: usize, b: usize) -> usize {
    a * NS + b
}

/// Number of homologs, re-exported for the search.
pub const HOMOLOG_COUNT: usize = HOMOLOGS;
