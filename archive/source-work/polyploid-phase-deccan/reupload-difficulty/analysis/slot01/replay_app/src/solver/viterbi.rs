
//! Viterbi over the hundred joint inheritance states.
//!
//! With the parental alleles fixed, the progeny are independent and each one is
//! a chain over the markers: the emission at a marker depends only on the joint
//! state there, and the transition depends only on the two joint states either
//! side of an interval. The transition kernel is separable — the two parents
//! recombine independently — so a step is two ten by ten max-plus products
//! rather than one hundred by hundred one.

use crate::cross_input::Cross;
use crate::cross_model::STATES;
use crate::solver::tables::{Tables, JOINTS};

/// One max-plus step of the chain.
///
/// `out[s'][t'] = max over (s, t) of fwd[s][t] + ta[s][s'] + tb[t][t']`, where
/// `ta` and `tb` are the two parents' transition log terms over the interval.
pub fn maxplus(
    fwd: &[f64; JOINTS],
    ta: &[f64; STATES * STATES],
    tb: &[f64; STATES * STATES],
    out: &mut [f64; JOINTS],
) {
    // Fold parent B's transition in first: `w[s][t']` keeps, for each parent A
    // state `s`, the best value reachable for each parent B state `t'`.
    let mut w = [f64::NEG_INFINITY; JOINTS];
    for s in 0..STATES {
        let base = s * STATES;
        for tp in 0..STATES {
            let mut best = f64::NEG_INFINITY;
            for t in 0..STATES {
                let v = fwd[base + t] + tb[t * STATES + tp];
                if v > best {
                    best = v;
                }
            }
            w[base + tp] = best;
        }
    }
    for sp in 0..STATES {
        for tp in 0..STATES {
            let mut best = f64::NEG_INFINITY;
            for s in 0..STATES {
                let v = w[s * STATES + tp] + ta[s * STATES + sp];
                if v > best {
                    best = v;
                }
            }
            out[sp * STATES + tp] = best;
        }
    }
}

/// As [`maxplus`], also recording where each maximum came from.
pub fn maxplus_track(
    fwd: &[f64; JOINTS],
    ta: &[f64; STATES * STATES],
    tb: &[f64; STATES * STATES],
    out: &mut [f64; JOINTS],
    back: &mut [u8; JOINTS],
) {
    let mut w = [f64::NEG_INFINITY; JOINTS];
    let mut wt = [0u8; JOINTS];
    for s in 0..STATES {
        let base = s * STATES;
        for tp in 0..STATES {
            let mut best = f64::NEG_INFINITY;
            let mut arg = 0u8;
            for t in 0..STATES {
                let v = fwd[base + t] + tb[t * STATES + tp];
                if v > best {
                    best = v;
                    arg = t as u8;
                }
            }
            w[base + tp] = best;
            wt[base + tp] = arg;
        }
    }
    for sp in 0..STATES {
        for tp in 0..STATES {
            let mut best = f64::NEG_INFINITY;
            let mut arg = 0u8;
            for s in 0..STATES {
                let v = w[s * STATES + tp] + ta[s * STATES + sp];
                if v > best {
                    best = v;
                    arg = s as u8;
                }
            }
            let j = sp * STATES + tp;
            out[j] = best;
            back[j] = arg * STATES as u8 + wt[usize::from(arg) * STATES + tp];
        }
    }
}

/// Everything computable for one progeny against a fixed parental assignment.
pub struct Chain<'a> {
    pub cross: &'a Cross,
    pub tables: &'a Tables,
}

impl<'a> Chain<'a> {
    pub fn new(cross: &'a Cross, tables: &'a Tables) -> Chain<'a> {
        Chain { cross, tables }
    }

    /// Writes the emission log terms of every joint state at one marker.
    pub fn emission(
        &self,
        progeny: usize,
        marker: usize,
        mask_a: u8,
        mask_b: u8,
        out: &mut [f64; JOINTS],
    ) {
        let gl = self.cross.child_gl[progeny][marker];
        let ua = &self.tables.u_of[usize::from(mask_a)];
        let ub = &self.tables.u_of[usize::from(mask_b)];
        let legal = &self.tables.legj[marker];
        for sa in 0..STATES {
            for sb in 0..STATES {
                let j = sa * STATES + sb;
                out[j] = if legal[j] {
                    gl[usize::from(ua[sa] + ub[sb])]
                } else {
                    f64::NEG_INFINITY
                };
            }
        }
    }

    /// Column masks of the chosen pair at `marker`.
    fn masks(&self, marker: usize, picks: &[(usize, usize)]) -> (u8, u8) {
        (
            self.tables.cols[0][marker][picks[marker].0],
            self.tables.cols[1][marker][picks[marker].1],
        )
    }

    /// Forward vectors: `fwd[m][j]` is the best score of markers `0..=m` for a
    /// path ending in joint state `j` at marker `m`.
    pub fn forward(&self, progeny: usize, picks: &[(usize, usize)]) -> Vec<[f64; JOINTS]> {
        let markers = self.tables.markers;
        let mut out = Vec::with_capacity(markers);
        let mut current = [f64::NEG_INFINITY; JOINTS];
        let mut buffer = [f64::NEG_INFINITY; JOINTS];
        let (ma, mb) = self.masks(0, picks);
        self.emission(progeny, 0, ma, mb, &mut buffer);
        for j in 0..JOINTS {
            current[j] = buffer[j] + self.tables.drj[0][j];
        }
        out.push(current);
        for m in 1..markers {
            let ta = &self.tables.tmat[0][m - 1];
            let tb = &self.tables.tmat[1][m - 1];
            let mut next = [f64::NEG_INFINITY; JOINTS];
            maxplus(&current, ta, tb, &mut next);
            let (ma, mb) = self.masks(m, picks);
            self.emission(progeny, m, ma, mb, &mut buffer);
            for j in 0..JOINTS {
                next[j] += buffer[j] + self.tables.drj[m][j];
            }
            out.push(next);
            current = next;
        }
        out
    }

    /// Backward vectors: `back[m][j]` is the best score of markers `m..M` for a
    /// path starting in joint state `j` at marker `m`.
    pub fn backward(&self, progeny: usize, picks: &[(usize, usize)]) -> Vec<[f64; JOINTS]> {
        let markers = self.tables.markers;
        let mut out = vec![[f64::NEG_INFINITY; JOINTS]; markers];
        let mut buffer = [f64::NEG_INFINITY; JOINTS];
        let last = markers - 1;
        let (ma, mb) = self.masks(last, picks);
        self.emission(progeny, last, ma, mb, &mut buffer);
        for j in 0..JOINTS {
            out[last][j] = buffer[j] + self.tables.drj[last][j];
        }
        for m in (0..last).rev() {
            let ta = &self.tables.tmat[0][m];
            let tb = &self.tables.tmat[1][m];
            let mut here = [f64::NEG_INFINITY; JOINTS];
            maxplus(&out[m + 1], ta, tb, &mut here);
            let (ma, mb) = self.masks(m, picks);
            self.emission(progeny, m, ma, mb, &mut buffer);
            for j in 0..JOINTS {
                here[j] += buffer[j] + self.tables.drj[m][j];
            }
            out[m] = here;
        }
        out
    }

    /// Best total for one progeny, and the joint state path that reaches it.
    pub fn best_path(&self, progeny: usize, picks: &[(usize, usize)]) -> (f64, Vec<usize>) {
        let markers = self.tables.markers;
        let mut forward: Vec<[f64; JOINTS]> = Vec::with_capacity(markers);
        let mut links: Vec<[u8; JOINTS]> = Vec::with_capacity(markers);
        let mut current = [f64::NEG_INFINITY; JOINTS];
        let mut buffer = [f64::NEG_INFINITY; JOINTS];
        let (ma, mb) = self.masks(0, picks);
        self.emission(progeny, 0, ma, mb, &mut buffer);
        for j in 0..JOINTS {
            current[j] = buffer[j] + self.tables.drj[0][j];
        }
        forward.push(current);
        links.push([0; JOINTS]);
        for m in 1..markers {
            let ta = &self.tables.tmat[0][m - 1];
            let tb = &self.tables.tmat[1][m - 1];
            let mut next = [f64::NEG_INFINITY; JOINTS];
            let mut back = [0u8; JOINTS];
            maxplus_track(&current, ta, tb, &mut next, &mut back);
            let (ma, mb) = self.masks(m, picks);
            self.emission(progeny, m, ma, mb, &mut buffer);
            for j in 0..JOINTS {
                next[j] += buffer[j] + self.tables.drj[m][j];
            }
            forward.push(next);
            links.push(back);
            current = next;
        }

        let mut cursor = 0usize;
        let mut best = f64::NEG_INFINITY;
        for j in 0..JOINTS {
            let v = current[j];
            if v > best {
                best = v;
                cursor = j;
            }
        }
        let mut path = vec![0usize; markers];
        for m in (0..markers).rev() {
            path[m] = cursor;
            if m > 0 {
                cursor = usize::from(links[m][cursor]);
            }
        }
        (best, path)
    }

    /// Best total for one progeny.
    pub fn best(&self, progeny: usize, picks: &[(usize, usize)]) -> f64 {
        let markers = self.tables.markers;
        let mut current = [f64::NEG_INFINITY; JOINTS];
        let mut buffer = [f64::NEG_INFINITY; JOINTS];
        let (ma, mb) = self.masks(0, picks);
        self.emission(progeny, 0, ma, mb, &mut buffer);
        for j in 0..JOINTS {
            current[j] = buffer[j] + self.tables.drj[0][j];
        }
        for m in 1..markers {
            let ta = &self.tables.tmat[0][m - 1];
            let tb = &self.tables.tmat[1][m - 1];
            let mut next = [f64::NEG_INFINITY; JOINTS];
            maxplus(&current, ta, tb, &mut next);
            let (ma, mb) = self.masks(m, picks);
            self.emission(progeny, m, ma, mb, &mut buffer);
            for j in 0..JOINTS {
                next[j] += buffer[j] + self.tables.drj[m][j];
            }
            current = next;
        }
        let mut best = f64::NEG_INFINITY;
        for v in current.iter() {
            if *v > best {
                best = *v;
            }
        }
        best
    }
}

/// Suffix bounds used to prune the search over parental assignments.
///
/// For each progeny these hold, per marker and joint state, the most any
/// continuation from that point can be worth once the parental columns there are
/// still free: the emission is replaced by its largest admissible value and the
/// double reduction and transition terms are exact.
pub struct SuffixBounds {
    /// `[progeny][marker][joint]` forward value at `marker` under the same
    /// relaxed emissions: the best of markers `0..=marker` ending in that joint
    /// state.
    pub fwd: Vec<Vec<[f64; JOINTS]>>,
    /// `[progeny][marker][joint]` best continuation from `marker` onwards.
    pub back: Vec<Vec<[f64; JOINTS]>>,
    /// `[progeny][marker][joint]` best continuation from `marker` onwards given
    /// the joint state at `marker - 1`, transition included.
    pub cont: Vec<Vec<[f64; JOINTS]>>,
    /// `[progeny]` best possible total for the whole chain.
    pub total: Vec<f64>,
}

impl SuffixBounds {
    /// Builds the bounds against a per marker emission ceiling. It is a
    /// parameter rather than the table's own because tightening which columns a
    /// marker admits tightens the ceiling, and that is worth rebuilding for.
    pub fn build(cross: &Cross, tables: &Tables, ub: &[Vec<[f64; JOINTS]>]) -> SuffixBounds {
        let markers = tables.markers;
        let mut back = Vec::with_capacity(cross.offspring);
        let mut cont = Vec::with_capacity(cross.offspring);
        let mut fwd = Vec::with_capacity(cross.offspring);
        let mut total = Vec::with_capacity(cross.offspring);

        for c in 0..cross.offspring {
            let mut rows = vec![[f64::NEG_INFINITY; JOINTS]; markers];
            for j in 0..JOINTS {
                rows[markers - 1][j] = ub[c][markers - 1][j] + tables.drj[markers - 1][j];
            }
            for m in (0..markers - 1).rev() {
                let ta = &tables.tmat[0][m];
                let tb = &tables.tmat[1][m];
                let mut here = [f64::NEG_INFINITY; JOINTS];
                maxplus(&rows[m + 1], ta, tb, &mut here);
                for j in 0..JOINTS {
                    here[j] += ub[c][m][j] + tables.drj[m][j];
                }
                rows[m] = here;
            }

            let mut front = vec![[f64::NEG_INFINITY; JOINTS]; markers];
            for j in 0..JOINTS {
                front[0][j] = ub[c][0][j] + tables.drj[0][j];
            }
            for m in 1..markers {
                let ta = &tables.tmat[0][m - 1];
                let tb = &tables.tmat[1][m - 1];
                let mut here = [f64::NEG_INFINITY; JOINTS];
                maxplus(&front[m - 1], ta, tb, &mut here);
                for j in 0..JOINTS {
                    here[j] += ub[c][m][j] + tables.drj[m][j];
                }
                front[m] = here;
            }
            let mut best = f64::NEG_INFINITY;
            for v in rows[0].iter() {
                if *v > best {
                    best = *v;
                }
            }
            total.push(best);

            let mut cs = vec![[f64::NEG_INFINITY; JOINTS]; markers];
            for m in 1..markers {
                let ta = &tables.tmat[0][m - 1];
                let tb = &tables.tmat[1][m - 1];
                let mut here = [f64::NEG_INFINITY; JOINTS];
                maxplus(&rows[m], ta, tb, &mut here);
                cs[m] = here;
            }
            back.push(rows);
            cont.push(cs);
            fwd.push(front);
        }
        SuffixBounds { fwd, back, cont, total }
    }

    /// Largest total any assignment can reach: the bound before anything is
    /// fixed.
    pub fn root(&self) -> f64 {
        self.total.iter().sum()
    }
}
