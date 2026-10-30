
//! Admissible upper bounds on what the rest of the markers can contribute.
//!
//! Given a partial assignment and every progeny's forward vector at the last
//! assigned marker, the search needs an upper bound on the rest of the score
//! before it can abandon a branch.
//!
//! Both bounds here relax exactly one thing: that all progeny inherit from the
//! *same* parental assignment. `single` lets every progeny choose its own, which
//! is one Viterbi pass each and is cheap enough to evaluate at every node.
//! `pair` lets each *pair* of progeny choose its own, sharing it within the
//! pair, which tightens the bound considerably and costs a squared state space.
//! The search tries the cheap one first and only pays for the tight one when the
//! cheap one has not settled the question.
//!
//! Each bound is a suffix table over markers, so it is computed once and read at
//! every node: `r[m]` is the most that markers `m..` can still add, as a
//! function of the state at marker `m - 1`.

use crate::solver::model::{Model, NJ, NEG, NS};

/// Joint states of a pair of progeny.
pub const PAIR: usize = NJ * NJ;

/// Suffix bounds for one cross.
pub struct Bounds {
    /// `single[m][c][j]`, for `m` in `1..markers`.
    pub single: Vec<Vec<[f64; NJ]>>,
    /// Progeny paired up, each pair sharing an assignment in the tight bound.
    pub pairs: Vec<(usize, usize)>,
    /// `pair_r[m][pair][j1 * NJ + j2]`.
    pub pair_r: Vec<Vec<Vec<f64>>>,
    /// Set when the progeny count is odd.
    pub leftover: Option<usize>,
}

impl Bounds {
    pub fn new(model: &Model, use_pairs: bool) -> Bounds {
        let markers = model.markers;
        let offspring = model.offspring;

        let mut single: Vec<Vec<[f64; NJ]>> = Vec::with_capacity(markers);
        for m in 0..markers {
            let mut per_c: Vec<[f64; NJ]> = Vec::with_capacity(offspring);
            for c in 0..offspring {
                per_c.push([0.0f64; NJ]);
            }
            single.push(per_c);
        }
        for c in 0..offspring {
            // Q at the last marker, then back down to the first.
            let mut q = [0.0f64; NJ];
            for j in 0..NJ {
                q[j] = single_max_node(model, c, markers - 1, j);
            }
            for m in (1..markers).rev() {
                // r[m] = max over j' of (T[m-1][j][j'] + q[j'])
                let ta = &model.ta[m - 1];
                let tb = &model.tb[m - 1];
                let row = &mut single[m][c];
                for j in 0..NJ {
                    let (a, b) = (j / NS, j % NS);
                    let mut best = NEG;
                    for ap in 0..NS {
                        let ta_v = ta[a * NS + ap];
                        for bp in 0..NS {
                            let v = ta_v + tb[b * NS + bp] + q[ap * NS + bp];
                            if v > best {
                                best = v;
                            }
                        }
                    }
                    row[j] = best;
                }
                if m > 1 {
                    // q becomes Q at marker m - 1
                    for j in 0..NJ {
                        q[j] = single_max_node(model, c, m - 1, j) + row[j];
                    }
                }
            }
        }

        let mut pairs: Vec<(usize, usize)> = Vec::new();
        let mut leftover = None;
        if use_pairs && offspring >= 2 {
            let mut c = 0usize;
            while c + 1 < offspring {
                pairs.push((c, c + 1));
                c += 2;
            }
            if c < offspring {
                leftover = Some(c);
            }
        } else {
            for c in 0..offspring {
                leftover = Some(c);
                break;
            }
        }

        let mut pair_r: Vec<Vec<Vec<f64>>> = Vec::with_capacity(markers);
        if !pairs.is_empty() {
            for _ in 0..markers {
                let mut per_pair: Vec<Vec<f64>> = Vec::with_capacity(pairs.len());
                for _ in 0..pairs.len() {
                    per_pair.push(vec![0.0f64; PAIR]);
                }
                pair_r.push(per_pair);
            }
            let mut x = vec![0.0f64; PAIR];
            let mut w1 = vec![0.0f64; PAIR];
            let mut w2 = vec![0.0f64; PAIR];
            let mut w3 = vec![0.0f64; PAIR];
            for (pi, &(c1, c2)) in pairs.iter().enumerate() {
                // Q at the last marker for this pair.
                for j in 0..PAIR {
                    x[j] = pair_max_node(model, c1, c2, markers - 1, j);
                }
                for m in (1..markers).rev() {
                    let ta = &model.ta[m - 1];
                    let tb = &model.tb[m - 1];
                    // y[j] = max over j' (T[j][j'] + x[j']) with T separable over
                    // the four parent state axes.
                    for a1 in 0..NS {
                        for b1p in 0..NS {
                            for a2p in 0..NS {
                                for b2p in 0..NS {
                                    let mut best = NEG;
                                    for a1p in 0..NS {
                                        let v =
                                            x[(((a1p * NS + b1p) * NS + a2p) * NS + b2p)] + ta[a1 * NS + a1p];
                                        if v > best {
                                            best = v;
                                        }
                                    }
                                    w1[(((a1 * NS + b1p) * NS + a2p) * NS + b2p)] = best;
                                }
                            }
                        }
                    }
                    for a1 in 0..NS {
                        for b1 in 0..NS {
                            for a2p in 0..NS {
                                for b2p in 0..NS {
                                    let mut best = NEG;
                                    for b1p in 0..NS {
                                        let v = w1[(((a1 * NS + b1p) * NS + a2p) * NS + b2p)]
                                            + tb[b1 * NS + b1p];
                                        if v > best {
                                            best = v;
                                        }
                                    }
                                    w2[(((a1 * NS + b1) * NS + a2p) * NS + b2p)] = best;
                                }
                            }
                        }
                    }
                    for a1 in 0..NS {
                        for b1 in 0..NS {
                            for a2 in 0..NS {
                                for b2p in 0..NS {
                                    let mut best = NEG;
                                    for a2p in 0..NS {
                                        let v = w2[(((a1 * NS + b1) * NS + a2p) * NS + b2p)]
                                            + ta[a2 * NS + a2p];
                                        if v > best {
                                            best = v;
                                        }
                                    }
                                    w3[(((a1 * NS + b1) * NS + a2) * NS + b2p)] = best;
                                }
                            }
                        }
                    }
                    let row = &mut pair_r[m][pi];
                    for a1 in 0..NS {
                        for b1 in 0..NS {
                            for a2 in 0..NS {
                                for b2 in 0..NS {
                                    let mut best = NEG;
                                    for b2p in 0..NS {
                                        let v = w3[(((a1 * NS + b1) * NS + a2) * NS + b2p)]
                                            + tb[b2 * NS + b2p];
                                        if v > best {
                                            best = v;
                                        }
                                    }
                                    row[(((a1 * NS + b1) * NS + a2) * NS + b2)] = best;
                                }
                            }
                        }
                    }
                    if m > 1 {
                        for j in 0..PAIR {
                            x[j] = pair_max_node(model, c1, c2, m - 1, j) + row[j];
                        }
                    }
                }
            }
        }

        Bounds { single, pairs, pair_r, leftover }
    }

    pub fn has_pairs(&self) -> bool {
        !self.pairs.is_empty()
    }

    /// Cheap bound: every progeny picks its own assignment from here on.
    pub fn bound_single(&self, depth: usize, fwd: &[Vec<[f64; NJ]>]) -> f64 {
        let m = depth + 1;
        let mut total = 0.0f64;
        if m >= self.single.len() {
            return total;
        }
        let row = &self.single[m];
        for c in 0..fwd.len() {
            let f = &fwd[c];
            let r = &row[c];
            let mut best = NEG;
            for j in 0..NJ {
                let v = f[j] + r[j];
                if v > best {
                    best = v;
                }
            }
            total += best;
        }
        total
    }

    /// Tight bound: each pair shares an assignment, and so does any leftover
    /// progeny with itself.
    pub fn bound_pair(&self, depth: usize, fwd: &[Vec<[f64; NJ]>]) -> f64 {
        let m = depth + 1;
        if m >= self.single.len() {
            return 0.0;
        }
        let mut total = 0.0f64;
        for (pi, &(c1, c2)) in self.pairs.iter().enumerate() {
            let f1 = &fwd[c1];
            let f2 = &fwd[c2];
            let r = &self.pair_r[m][pi];
            let mut best = NEG;
            for j1 in 0..NJ {
                let v1 = f1[j1];
                let base = j1 * NJ;
                for j2 in 0..NJ {
                    let v = v1 + f2[j2] + r[base + j2];
                    if v > best {
                        best = v;
                    }
                }
            }
            total += best;
        }
        if let Some(c) = self.leftover {
            let f = &fwd[c];
            let r = &self.single[m][c];
            let mut best = NEG;
            for j in 0..NJ {
                let v = f[j] + r[j];
                if v > best {
                    best = v;
                }
            }
            total += best;
        }
        total
    }
}

/// Most one progeny can score at `m` from state `j` over every assignment there.
fn single_max_node(model: &Model, c: usize, m: usize, j: usize) -> f64 {
    let base = model.base[m][j];
    if base == NEG {
        return NEG;
    }
    let gl = &model.gl[c][m];
    let mut best = NEG;
    for z in 0..model.zcount[m] {
        let v = gl[usize::from(model.dose[m][z][j])];
        if v > best {
            best = v;
        }
    }
    base + best
}

/// Most a pair can score together at `m` from states `j1`, `j2`.
fn pair_max_node(model: &Model, c1: usize, c2: usize, m: usize, j: usize) -> f64 {
    let (j1, j2) = (j / NJ, j % NJ);
    let base = model.base[m][j1] + model.base[m][j2];
    if base == NEG {
        return NEG;
    }
    let g1 = &model.gl[c1][m];
    let g2 = &model.gl[c2][m];
    let mut best = NEG;
    for z in 0..model.zcount[m] {
        let row = &model.dose[m][z];
        let v = g1[usize::from(row[j1])] + g2[usize::from(row[j2])];
        if v > best {
            best = v;
        }
    }
    base + best
}
