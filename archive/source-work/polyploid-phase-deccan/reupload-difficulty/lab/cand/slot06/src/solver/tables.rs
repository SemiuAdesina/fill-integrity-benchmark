
//! Precomputed tables and the per progeny Viterbi pass.
//!
//! Everything the search touches repeatedly is tabulated once: the admissible
//! allele columns at each marker, the double reduction log term of every state
//! at every marker, the transition log term of every state pair at every
//! interval, and the dosage each joint state produces under each column pair.
//!
//! The Viterbi pass itself is max plus. The joint state is a pair of parental
//! states, a hundred of them, but the transition term splits into a part for
//! parent A and a part for parent B, so a step costs two `10 * 10 * 10` sweeps
//! instead of one `100 * 100` sweep.

use crate::cross_input::Cross;
use crate::cross_model::{Parent, ALL_STATES};

/// Inheritance states for one parent.
pub const NS: usize = 10;
/// Joint inheritance states, one per parent: `sa * NS + sb`.
pub const NJ: usize = 100;

/// Tabulated cross, ready for repeated scoring.
pub struct Ctx {
    pub m: usize,
    pub n: usize,
    /// Number of admissible columns for `[parent][marker]`.
    pub ncol: [Vec<usize>; 2],
    /// The admissible columns themselves, as four bit masks.
    pub cols: [Vec<Vec<u8>>; 2],
    /// Double reduction log term `[parent][marker][state]`, `-inf` when illegal.
    pub drv: [Vec<[f64; NS]>; 2],
    /// Transition log term `[parent][interval][from * NS + to]`.
    pub tr: [Vec<[f64; NS * NS]>; 2],
    /// Dosage each joint state yields: `[marker][ia * ncol_b + ib][joint]`.
    pub dose: Vec<Vec<[u8; NJ]>>,
    /// Read evidence `[progeny][marker][dosage]`.
    pub gl: Vec<Vec<[f64; 5]>>,
}

impl Ctx {
    pub fn new(cross: &Cross) -> Ctx {
        let m = cross.markers;
        let n = cross.offspring;

        let mut ncol = [Vec::with_capacity(m), Vec::with_capacity(m)];
        let mut cols = [Vec::with_capacity(m), Vec::with_capacity(m)];
        let mut drv = [Vec::with_capacity(m), Vec::with_capacity(m)];
        let mut tr = [Vec::with_capacity(m), Vec::with_capacity(m)];

        for (pi, parent) in [Parent::A, Parent::B].iter().enumerate() {
            let params = cross.params(*parent);
            for mm in 0..m {
                let mut admissible = Vec::new();
                for mask in 0u8..16 {
                    if mask.count_ones() as u8 == params.dosage[mm] {
                        admissible.push(mask);
                    }
                }
                ncol[pi].push(admissible.len());
                cols[pi].push(admissible);

                let mut term = [0.0f64; NS];
                for (si, state) in ALL_STATES.iter().enumerate() {
                    term[si] = if state.is_double_reduction() {
                        if params.dr_allowed[mm] {
                            params.dr_rate[mm].ln()
                        } else {
                            f64::NEG_INFINITY
                        }
                    } else {
                        (1.0 - params.dr_rate[mm]).ln()
                    };
                }
                drv[pi].push(term);
            }

            for iv in 0..m - 1 {
                let r = params.recombination[iv];
                let ln_r = r.ln();
                let ln_1r = (1.0 - r).ln();
                let mut term = [0.0f64; NS * NS];
                for (i, from) in ALL_STATES.iter().enumerate() {
                    for (j, to) in ALL_STATES.iter().enumerate() {
                        let k = f64::from(from.switch_count(*to));
                        term[i * NS + j] = k * ln_r + (2.0 - k) * ln_1r;
                    }
                }
                tr[pi].push(term);
            }
        }

        let mut dose = Vec::with_capacity(m);
        for mm in 0..m {
            let na = ncol[0][mm];
            let nb = ncol[1][mm];
            let mut table = Vec::with_capacity(na * nb);
            for ia in 0..na {
                let ma = cols[0][mm][ia];
                for ib in 0..nb {
                    let mb = cols[1][mm][ib];
                    let mut row = [0u8; NJ];
                    for sa in 0..NS {
                        let state = ALL_STATES[sa];
                        let ga = ((ma >> state.low()) & 1) + ((ma >> state.high()) & 1);
                        for sb in 0..NS {
                            let other = ALL_STATES[sb];
                            let gb = ((mb >> other.low()) & 1) + ((mb >> other.high()) & 1);
                            row[sa * NS + sb] = ga + gb;
                        }
                    }
                    table.push(row);
                }
            }
            dose.push(table);
        }

        Ctx {
            m,
            n,
            ncol,
            cols,
            drv,
            tr,
            dose,
            gl: cross.child_gl.clone(),
        }
    }

    /// Total log likelihood of one progeny given the columns, maximised over
    /// paths. Forward pass only; no backpointers and no backward pass.
    pub fn best_value(&self, c: usize, ca: &[u8], cb: &[u8]) -> f64 {
        let m = self.m;
        let gl = &self.gl[c];
        let mut cur = [f64::NEG_INFINITY; NJ];
        {
            let d = &self.dose[0][ca[0] as usize * self.ncol[1][0] + cb[0] as usize];
            let da = self.drv[0][0];
            let db = self.drv[1][0];
            for j in 0..NJ {
                cur[j] = gl[0][d[j] as usize] + da[j / NS] + db[j % NS];
            }
        }
        for mm in 1..m {
            let ta = &self.tr[0][mm - 1];
            let tb = &self.tr[1][mm - 1];
            let mut mid = [f64::NEG_INFINITY; NJ];
            for sa in 0..NS {
                for sb in 0..NS {
                    let mut best = f64::NEG_INFINITY;
                    let base = sa * NS;
                    for prev in 0..NS {
                        let v = cur[base + prev] + tb[prev * NS + sb];
                        if v > best {
                            best = v;
                        }
                    }
                    mid[base + sb] = best;
                }
            }
            let mut next = [f64::NEG_INFINITY; NJ];
            for sb in 0..NS {
                for sa in 0..NS {
                    let mut best = f64::NEG_INFINITY;
                    for prev in 0..NS {
                        let v = mid[prev * NS + sb] + ta[prev * NS + sa];
                        if v > best {
                            best = v;
                        }
                    }
                    next[sa * NS + sb] = best;
                }
            }
            let d = &self.dose[mm][ca[mm] as usize * self.ncol[1][mm] + cb[mm] as usize];
            let da = self.drv[0][mm];
            let db = self.drv[1][mm];
            for j in 0..NJ {
                cur[j] = next[j] + gl[mm][d[j] as usize] + da[j / NS] + db[j % NS];
            }
        }
        let mut best = f64::NEG_INFINITY;
        for v in cur.iter() {
            if *v > best {
                best = *v;
            }
        }
        best
    }

    /// Full Viterbi for one progeny: forward values, backward values and the
    /// backpointers needed to read a best path back off.
    pub fn viterbi(&self, c: usize, ca: &[u8], cb: &[u8]) -> Prog {
        let m = self.m;
        let gl = &self.gl[c];

        let mut node: Vec<[f64; NJ]> = Vec::with_capacity(m);
        for mm in 0..m {
            let d = &self.dose[mm][ca[mm] as usize * self.ncol[1][mm] + cb[mm] as usize];
            let da = self.drv[0][mm];
            let db = self.drv[1][mm];
            let mut row = [0.0f64; NJ];
            for j in 0..NJ {
                row[j] = gl[mm][d[j] as usize] + da[j / NS] + db[j % NS];
            }
            node.push(row);
        }

        let mut fwd: Vec<[f64; NJ]> = Vec::with_capacity(m);
        // Backpointer row `mm` belongs to the step into marker `mm`, so row 0
        // is a placeholder and the real rows start at index one.
        let mut bpa: Vec<[u8; NJ]> = vec![[0u8; NJ]];
        let mut bpb: Vec<[u8; NJ]> = vec![[0u8; NJ]];

        fwd.push(node[0]);
        for mm in 1..m {
            let ta = &self.tr[0][mm - 1];
            let tb = &self.tr[1][mm - 1];
            let prev = fwd[mm - 1];
            let mut mid = [f64::NEG_INFINITY; NJ];
            let mut pbpb = [0u8; NJ];
            for sa in 0..NS {
                for sb in 0..NS {
                    let mut best = f64::NEG_INFINITY;
                    let mut arg = 0u8;
                    let base = sa * NS;
                    for p in 0..NS {
                        let v = prev[base + p] + tb[p * NS + sb];
                        if v > best {
                            best = v;
                            arg = p as u8;
                        }
                    }
                    mid[base + sb] = best;
                    pbpb[base + sb] = arg;
                }
            }
            let mut next = [f64::NEG_INFINITY; NJ];
            let pbpa = {
                let mut out = [0u8; NJ];
                for sb in 0..NS {
                    for sa in 0..NS {
                        let mut best = f64::NEG_INFINITY;
                        let mut arg = 0u8;
                        for p in 0..NS {
                            let v = mid[p * NS + sb] + ta[p * NS + sa];
                            if v > best {
                                best = v;
                                arg = p as u8;
                            }
                        }
                        next[sa * NS + sb] = best;
                        out[sa * NS + sb] = arg;
                    }
                }
                out
            };
            for j in 0..NJ {
                next[j] += node[mm][j];
            }
            fwd.push(next);
            bpa.push(pbpa);
            bpb.push(pbpb);
        }

        let mut val = f64::NEG_INFINITY;
        let mut end = 0usize;
        for (j, v) in fwd[m - 1].iter().enumerate() {
            if *v > val {
                val = *v;
                end = j;
            }
        }

        let mut bwd: Vec<[f64; NJ]> = vec![[0.0f64; NJ]; m];
        bwd[m - 1] = node[m - 1];
        for mm in (0..m - 1).rev() {
            let ta = &self.tr[0][mm];
            let tb = &self.tr[1][mm];
            let later = bwd[mm + 1];
            // inner[sa][sb'] = max over sa' of ta[sa][sa'] + later[sa'][sb']
            let mut inner = [f64::NEG_INFINITY; NJ];
            for sa in 0..NS {
                for sb in 0..NS {
                    let mut best = f64::NEG_INFINITY;
                    for sa2 in 0..NS {
                        let v = ta[sa * NS + sa2] + later[sa2 * NS + sb];
                        if v > best {
                            best = v;
                        }
                    }
                    inner[sa * NS + sb] = best;
                }
            }
            let mut row = [0.0f64; NJ];
            for sa in 0..NS {
                for sb in 0..NS {
                    let mut best = f64::NEG_INFINITY;
                    for sb2 in 0..NS {
                        let v = tb[sb * NS + sb2] + inner[sa * NS + sb2];
                        if v > best {
                            best = v;
                        }
                    }
                    row[sa * NS + sb] = best + node[mm][sa * NS + sb];
                }
            }
            bwd[mm] = row;
        }

        Prog { val, end: end as u8, fwd, bwd, bpa, bpb }
    }

    /// Total over every progeny for the given columns.
    pub fn total(&self, ca: &[u8], cb: &[u8]) -> f64 {
        let mut sum = 0.0f64;
        for c in 0..self.n {
            sum += self.best_value(c, ca, cb);
        }
        sum
    }
}

/// One progeny's Viterbi result.
pub struct Prog {
    /// Value of a best path.
    pub val: f64,
    /// Joint state index at the last marker on a best path.
    pub end: u8,
    /// Forward values, `fwd[marker][joint]`, node included.
    pub fwd: Vec<[f64; NJ]>,
    /// Backward values, `bwd[marker][joint]`, node included.
    pub bwd: Vec<[f64; NJ]>,
    /// `bpa[marker][sa * NS + sb]`: predecessor parent A state.
    pub bpa: Vec<[u8; NJ]>,
    /// `bpb[marker][sa * NS + sb]`: predecessor parent B state, given the A one.
    pub bpb: Vec<[u8; NJ]>,
}

impl Prog {
    /// A best path, as one joint state index per marker.
    pub fn path(&self) -> Vec<usize> {
        let m = self.fwd.len();
        let mut out = vec![0usize; m];
        let mut j = usize::from(self.end);
        out[m - 1] = j;
        for mm in (1..m).rev() {
            let sb = j % NS;
            let pa = usize::from(self.bpa[mm][j]);
            let pb = usize::from(self.bpb[mm][pa * NS + sb]);
            j = pa * NS + pb;
            out[mm - 1] = j;
        }
        out
    }
}
