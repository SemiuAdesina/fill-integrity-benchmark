//! Viterbi over the hundred joint inheritance states.
//!
//! The transition factorises per parent, so one step is two ten by ten
//! max-plus products rather than one hundred by one hundred. Backward passes
//! reuse the same routine with the transposed transition blocks.

use crate::cross_model::{Haplotypes, Parent, ProgenyPath, ALL_STATES};

use super::model::{Model, NJ, NS};

/// `out[sA'][sB'] = max over (sA, sB) of f[sA][sB] + ta[sA][sA'] + tb[sB][sB']`.
pub fn propagate(f: &[f64; NJ], ta: &[[f64; NS]; NS], tb: &[[f64; NS]; NS], out: &mut [f64; NJ]) {
    let mut mid = [[f64::NEG_INFINITY; NS]; NS];
    for sb in 0..NS {
        for sa_to in 0..NS {
            let mut best = f64::NEG_INFINITY;
            for sa in 0..NS {
                let v = f[sa * NS + sb] + ta[sa][sa_to];
                if v > best {
                    best = v;
                }
            }
            mid[sb][sa_to] = best;
        }
    }
    for sa_to in 0..NS {
        for sb_to in 0..NS {
            let mut best = f64::NEG_INFINITY;
            for sb in 0..NS {
                let v = mid[sb][sa_to] + tb[sb][sb_to];
                if v > best {
                    best = v;
                }
            }
            out[sa_to * NS + sb_to] = best;
        }
    }
}

impl Model {
    /// Index into `self.pairs[marker]` of a column index pair.
    #[inline]
    pub fn pair_index(&self, marker: usize, cols: (usize, usize)) -> usize {
        cols.0 * self.cols[1][marker].len() + cols.1
    }

    /// Emission of every joint state for one progeny at one marker.
    #[inline]
    pub fn emission_row(&self, progeny: usize, marker: usize, pair: usize, out: &mut [f64; NJ]) {
        let row = &self.dose[marker][pair];
        let gl = &self.gl[progeny][marker];
        for s in 0..NJ {
            out[s] = gl[usize::from(row[s])];
        }
    }

    /// Emission plus double reduction weight of every joint state.
    pub fn node_row(&self, progeny: usize, marker: usize, pair: usize, out: &mut [f64; NJ]) {
        self.emission_row(progeny, marker, pair, out);
        let drj = &self.drj[marker];
        for s in 0..NJ {
            out[s] += drj[s];
        }
    }
}

/// Best total for one progeny under a fixed column sequence.
pub fn best_total(model: &Model, progeny: usize, cols: &[(usize, usize)]) -> f64 {
    let m = model.m;
    let mut f = [f64::NEG_INFINITY; NJ];
    model.node_row(progeny, 0, model.pair_index(0, cols[0]), &mut f);
    let mut u = [f64::NEG_INFINITY; NJ];
    for mm in 1..m {
        propagate(&f, &model.trans[0][mm - 1], &model.trans[1][mm - 1], &mut u);
        model.node_row(progeny, mm, model.pair_index(mm, cols[mm]), &mut f);
        for s in 0..NJ {
            f[s] += u[s];
        }
    }
    let mut best = f64::NEG_INFINITY;
    for v in f {
        if v > best {
            best = v;
        }
    }
    best
}

/// Total over every progeny under a fixed column sequence.
pub fn total_all(model: &Model, cols: &[(usize, usize)]) -> f64 {
    let mut total = 0.0;
    for c in 0..model.n {
        total += best_total(model, c, cols);
    }
    total
}

/// Forward and backward tables for one progeny under fixed columns.
///
/// `fwd[mm][s]` covers markers `0..=mm` ending in state `s`, `bwd[mm][s]`
/// covers markers `mm..M-1` starting in state `s`, and both include the node
/// term of marker `mm`, so `fwd[mm][s] + bwd[mm][s] - node[mm][s]` is the value
/// of the best path through `s`.
pub fn forward_backward(
    model: &Model,
    progeny: usize,
    cols: &[(usize, usize)],
) -> (Vec<[f64; NJ]>, Vec<[f64; NJ]>, Vec<[f64; NJ]>) {
    let m = model.m;
    let mut node = vec![[0.0f64; NJ]; m];
    for mm in 0..m {
        model.node_row(progeny, mm, model.pair_index(mm, cols[mm]), &mut node[mm]);
    }

    let mut fwd = vec![[f64::NEG_INFINITY; NJ]; m];
    fwd[0] = node[0];
    for mm in 1..m {
        let mut u = [f64::NEG_INFINITY; NJ];
        propagate(&fwd[mm - 1], &model.trans[0][mm - 1], &model.trans[1][mm - 1], &mut u);
        for s in 0..NJ {
            fwd[mm][s] = u[s] + node[mm][s];
        }
    }

    let mut bwd = vec![[f64::NEG_INFINITY; NJ]; m];
    bwd[m - 1] = node[m - 1];
    for mm in (0..m - 1).rev() {
        let mut w = [f64::NEG_INFINITY; NJ];
        propagate(&bwd[mm + 1], &model.trans_t[0][mm], &model.trans_t[1][mm], &mut w);
        for s in 0..NJ {
            bwd[mm][s] = node[mm][s] + w[s];
        }
    }
    (fwd, bwd, node)
}

/// Among the paths attaining the optimum, the one whose signature is smallest.
///
/// The signature is read in the order the result format demands: parent A's
/// whole state sequence first, then parent B's, each state contributing the
/// remapped `a` then the remapped `b`. `remap_a` and `remap_b` give, for each
/// canonical single parent state index, the pair of values its two entries take
/// after relabelling; candidates are tried in increasing order of that pair.
pub fn lexicographic_path(
    model: &Model,
    progeny: usize,
    cols: &[(usize, usize)],
    remap_a: &[(u8, u8)],
    remap_b: &[(u8, u8)],
) -> ProgenyPath {
    let m = model.m;
    let (fwd, bwd, node) = forward_backward(model, progeny, cols);

    let mut best = f64::NEG_INFINITY;
    for s in 0..NJ {
        if fwd[m - 1][s] > best {
            best = fwd[m - 1][s];
        }
    }
    let floor = best - 1e-8_f64;

    let mut order: Vec<usize> = (0..NS).collect();
    let order_a = {
        order.sort_by_key(|s| remap_a[*s]);
        order.clone()
    };
    let order_b = {
        order.sort_by_key(|s| remap_b[*s]);
        order.clone()
    };

    // Parent A first: the partial forward value over B states, with the A
    // states of markers `0..mm` already fixed.
    let mut chosen_a = vec![0usize; m];
    let mut fa = vec![[f64::NEG_INFINITY; NS]; m];
    for mm in 0..m {
        let mut picked = None;
        for &sa in &order_a {
            let mut row = [f64::NEG_INFINITY; NS];
            for sb in 0..NS {
                let joint = sa * NS + sb;
                row[sb] = if mm == 0 {
                    node[0][joint]
                } else {
                    let prev_sa = chosen_a[mm - 1];
                    let ta = model.trans[0][mm - 1][prev_sa][sa];
                    let mut best_prev = f64::NEG_INFINITY;
                    for sb_prev in 0..NS {
                        let v = fa[mm - 1][sb_prev]
                            + ta
                            + model.trans[1][mm - 1][sb_prev][sb];
                        if v > best_prev {
                            best_prev = v;
                        }
                    }
                    best_prev + node[mm][joint]
                };
            }
            let reachable = (0..NS).any(|sb| {
                let joint = sa * NS + sb;
                row[sb].is_finite() && row[sb] + bwd[mm][joint] - node[mm][joint] >= floor
            });
            if reachable {
                picked = Some((sa, row));
                break;
            }
        }
        let (sa, row) = picked.expect("the optimum is attainable");
        chosen_a[mm] = sa;
        fa[mm] = row;
    }

    // Then parent B, with A fixed.
    let mut chosen_b = vec![0usize; m];
    let mut fb = vec![f64::NEG_INFINITY; m];
    for mm in 0..m {
        let sa = chosen_a[mm];
        let mut picked = None;
        for &sb in &order_b {
            let joint = sa * NS + sb;
            let value = if mm == 0 {
                node[0][joint]
            } else {
                fb[mm - 1]
                    + model.trans[0][mm - 1][chosen_a[mm - 1]][sa]
                    + model.trans[1][mm - 1][chosen_b[mm - 1]][sb]
                    + node[mm][joint]
            };
            if value.is_finite() && value + bwd[mm][joint] - node[mm][joint] >= floor {
                picked = Some((sb, value));
                break;
            }
        }
        let (sb, value) = picked.expect("the optimum is attainable");
        chosen_b[mm] = sb;
        fb[mm] = value;
    }

    ProgenyPath {
        from_a: chosen_a.iter().map(|s| ALL_STATES[*s]).collect(),
        from_b: chosen_b.iter().map(|s| ALL_STATES[*s]).collect(),
    }
}


/// `propagate` recording, for each joint state, the predecessor it came from.
pub fn propagate_arg(
    f: &[f64; NJ],
    ta: &[[f64; NS]; NS],
    tb: &[[f64; NS]; NS],
    out: &mut [f64; NJ],
    back: &mut [usize; NJ],
) {
    let mut mid = [[f64::NEG_INFINITY; NS]; NS];
    let mut mid_arg = [[0usize; NS]; NS];
    for sb in 0..NS {
        for sa_to in 0..NS {
            let mut best = f64::NEG_INFINITY;
            let mut arg = 0usize;
            for sa in 0..NS {
                let v = f[sa * NS + sb] + ta[sa][sa_to];
                if v > best {
                    best = v;
                    arg = sa;
                }
            }
            mid[sb][sa_to] = best;
            mid_arg[sb][sa_to] = arg;
        }
    }
    for sa_to in 0..NS {
        for sb_to in 0..NS {
            let mut best = f64::NEG_INFINITY;
            let mut arg = 0usize;
            for sb in 0..NS {
                let v = mid[sb][sa_to] + tb[sb][sb_to];
                if v > best {
                    best = v;
                    arg = sb;
                }
            }
            out[sa_to * NS + sb_to] = best;
            back[sa_to * NS + sb_to] = mid_arg[arg][sa_to] * NS + arg;
        }
    }
}

/// A joint state at each marker on a path attaining the optimum.
pub fn best_path(model: &Model, progeny: usize, cols: &[(usize, usize)]) -> Vec<usize> {
    let m = model.m;
    let mut node = vec![[0.0f64; NJ]; m];
    for mm in 0..m {
        model.node_row(progeny, mm, model.pair_index(mm, cols[mm]), &mut node[mm]);
    }
    let mut fwd = vec![[f64::NEG_INFINITY; NJ]; m];
    let mut back = vec![[0usize; NJ]; m];
    fwd[0] = node[0];
    for mm in 1..m {
        let mut u = [f64::NEG_INFINITY; NJ];
        let mut b = [0usize; NJ];
        propagate_arg(&fwd[mm - 1], &model.trans[0][mm - 1], &model.trans[1][mm - 1], &mut u, &mut b);
        back[mm] = b;
        for s in 0..NJ {
            fwd[mm][s] = u[s] + node[mm][s];
        }
    }
    let mut state = 0usize;
    let mut best = f64::NEG_INFINITY;
    for s in 0..NJ {
        if fwd[m - 1][s] > best {
            best = fwd[m - 1][s];
            state = s;
        }
    }
    let mut path = vec![0usize; m];
    path[m - 1] = state;
    for mm in (1..m).rev() {
        state = back[mm][state];
        path[mm - 1] = state;
    }
    path
}

/// Haplotypes of one parent from a column index sequence.
pub fn haplotypes_from_cols(model: &Model, parent: Parent, cols: &[(usize, usize)]) -> Haplotypes {
    let p = parent.index();
    let mut haps = Haplotypes::zeros(model.m);
    for (mm, pair) in cols.iter().enumerate() {
        let index = if p == 0 { pair.0 } else { pair.1 };
        let mask = model.cols[p][mm][index];
        for h in 0..4 {
            haps.set(h, mm, (mask >> h) & 1);
        }
    }
    haps
}
