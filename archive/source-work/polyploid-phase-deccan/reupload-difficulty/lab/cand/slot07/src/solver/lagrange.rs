
//! A Lagrangian bound on the whole search.
//!
//! What makes the problem hard is one constraint: every progeny inherits from
//! the *same* parental assignment. Relax it by giving every progeny its own
//! copy of the assignment, then charge for disagreement through multipliers
//! `lambda[c][m][v]` on "progeny `c` uses column `v` at marker `m`". What is
//! left is one Viterbi pass per progeny.
//!
//! ```text
//! L(lambda) = sum_c max over (path, z^c) [ score_c(path, z^c)
//!                                          + sum_m lambda[c][m][z^c_m] ]
//!             - sum_m min_v ( sum_c lambda[c][m][v] )
//! ```
//!
//! is an upper bound on the optimum for every choice of multipliers, because a
//! feasible reconstruction has every copy equal and the added terms cancel.
//! Subgradient steps move the multipliers towards the cheapest bound.
//!
//! The result is then read as a suffix table, exactly like `bounds::Bounds`, so
//! the search can consult it at any depth: the multipliers are fixed once, but
//! the bound stays valid however the prefix was chosen.

use crate::solver::model::{Model, NJ, NEG, NS};

/// Multipliers, and the bound they certify.
pub struct Lagrange {
    /// `lambda[c][m][v]`.
    pub lambda: Vec<Vec<Vec<f64>>>,
    /// `L(lambda)` for the stored multipliers.
    pub value: f64,
}

impl Lagrange {
    /// Zero multipliers, which give the plain "every progeny picks its own"
    /// bound.
    pub fn new(model: &Model) -> Lagrange {
        let lambda = (0..model.offspring)
            .map(|c| {
                (0..model.markers)
                    .map(|m| vec![0.0f64; model.zcount[m]])
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        Lagrange { lambda, value: f64::INFINITY }
    }

    /// Node cost for progeny `c` at marker `m` and state `j`: the best its own
    /// copy of the assignment can do there, multiplier included. `arg` receives
    /// the column that achieved it.
    fn node_costs(
        &self,
        model: &Model,
        c: usize,
        m: usize,
        out: &mut [f64; NJ],
        arg: &mut [u8; NJ],
    ) -> bool {
        let gl = &model.gl[c][m];
        let lam = &self.lambda[c][m];
        let base = &model.base[m];
        for j in 0..NJ {
            if base[j] == NEG {
                out[j] = NEG;
                arg[j] = 0;
                continue;
            }
            let mut best = NEG;
            let mut at = 0usize;
            for (v, row) in model.dose[m].iter().enumerate() {
                let value = gl[usize::from(row[j])] + lam[v];
                if value > best {
                    best = value;
                    at = v;
                }
            }
            out[j] = best + base[j];
            arg[j] = at as u8;
        }
        true
    }

    /// `L(lambda)`, and the column each progeny would pick at each marker.
    pub fn evaluate(&self, model: &Model, picks: &mut Vec<Vec<usize>>) -> f64 {
        let markers = model.markers;
        let mut total = 0.0f64;
        let mut cur = [0.0f64; NJ];
        let mut next = [0.0f64; NJ];
        let mut mid = [0.0f64; NJ];
        let mut node = [0.0f64; NJ];
        let mut arg = [0u8; NJ];
        let mut back_a = vec![[0u8; NJ]; markers];
        let mut back_b = vec![[0u8; NJ]; markers];
        let mut arg_all = vec![[0u8; NJ]; markers];

        for c in 0..model.offspring {
            for m in 0..markers {
                self.node_costs(model, c, m, &mut node, &mut arg);
                arg_all[m] = arg;
                if m == 0 {
                    cur.copy_from_slice(&node);
                    continue;
                }
                let ta = &model.ta[m - 1];
                let tb = &model.tb[m - 1];
                let mut ba = [0u8; NJ];
                let mut bb = [0u8; NJ];
                for sp in 0..NS {
                    for b in 0..NS {
                        let mut best = NEG;
                        let mut at = 0usize;
                        for s in 0..NS {
                            let v = cur[s * NS + b] + ta[s * NS + sp];
                            if v > best {
                                best = v;
                                at = s;
                            }
                        }
                        mid[sp * NS + b] = best;
                        ba[sp * NS + b] = at as u8;
                    }
                }
                for sp in 0..NS {
                    for bp in 0..NS {
                        let mut best = NEG;
                        let mut at = 0usize;
                        for b in 0..NS {
                            let v = mid[sp * NS + b] + tb[b * NS + bp];
                            if v > best {
                                best = v;
                                at = b;
                            }
                        }
                        let j = sp * NS + bp;
                        next[j] = best + node[j];
                        bb[j] = at as u8;
                    }
                }
                cur.copy_from_slice(&next);
                back_a[m] = ba;
                back_b[m] = bb;
            }
            let mut best = NEG;
            let mut at = 0usize;
            for j in 0..NJ {
                if cur[j] > best {
                    best = cur[j];
                    at = j;
                }
            }
            total += best;
            let mut j = at;
            for m in (0..markers).rev() {
                picks[c][m] = usize::from(arg_all[m][j]);
                if m > 0 {
                    let sa = j / NS;
                    let pb = usize::from(back_b[m][j]);
                    let pa = usize::from(back_a[m][sa * NS + pb]);
                    j = pa * NS + pb;
                }
            }
        }

        for m in 0..markers {
            total -= self.cheapest(model, m);
        }
        total
    }

    fn cheapest(&self, model: &Model, m: usize) -> f64 {
        let mut low = f64::INFINITY;
        for v in 0..model.zcount[m] {
            let mut sum = 0.0f64;
            for c in 0..model.offspring {
                sum += self.lambda[c][m][v];
            }
            if sum < low {
                low = sum;
            }
        }
        low
    }

    /// Which column the consensus term prefers at `m`.
    fn consensus(&self, model: &Model, m: usize) -> usize {
        let mut low = f64::INFINITY;
        let mut at = 0usize;
        for v in 0..model.zcount[m] {
            let mut sum = 0.0f64;
            for c in 0..model.offspring {
                sum += self.lambda[c][m][v];
            }
            if sum < low {
                low = sum;
                at = v;
            }
        }
        at
    }

    /// Moves the multipliers towards the cheapest bound they can certify.
    ///
    /// `lower` is any achievable score, used only to size the steps.
    pub fn optimize(&mut self, model: &Model, lower: f64, rounds: usize) -> f64 {
        let markers = model.markers;
        let offspring = model.offspring;
        let mut picks = vec![vec![0usize; markers]; offspring];
        let mut best_value = f64::INFINITY;
        let mut best_lambda: Option<Vec<Vec<Vec<f64>>>> = None;
        let mut step_scale = 1.6f64;
        let mut since_improvement = 0usize;

        for _ in 0..rounds {
            let value = self.evaluate(model, &mut picks);
            if value < best_value - 1e-12 {
                best_value = value;
                best_lambda = Some(self.lambda.clone());
                since_improvement = 0;
            } else {
                since_improvement += 1;
                if since_improvement > 24 {
                    step_scale *= 0.6;
                    since_improvement = 0;
                    if step_scale < 1e-4 {
                        break;
                    }
                }
            }

            let gap = value - lower;
            if !(gap > 0.0) {
                break;
            }
            let mut norm = 0.0f64;
            let mut moves: Vec<(usize, usize, usize, usize)> = Vec::new();
            for m in 0..markers {
                let shared = self.consensus(model, m);
                for c in 0..offspring {
                    if picks[c][m] != shared {
                        norm += 2.0;
                        moves.push((c, m, picks[c][m], shared));
                    }
                }
            }
            if norm == 0.0 {
                break;
            }
            let step = step_scale * gap / norm;
            for (c, m, mine, shared) in moves {
                self.lambda[c][m][mine] -= step;
                self.lambda[c][m][shared] += step;
            }
        }

        if let Some(best) = best_lambda {
            self.lambda = best;
            self.value = best_value;
        } else {
            self.value = best_value;
        }
        best_value
    }

    /// Rebuilds the bound as a suffix table the search can read at any depth.
    pub fn suffix(&self, model: &Model) -> Suffix {
        let markers = model.markers;
        let offspring = model.offspring;
        let mut s: Vec<Vec<[f64; NJ]>> = Vec::with_capacity(offspring);
        let mut node = [0.0f64; NJ];
        let mut arg = [0u8; NJ];
        for c in 0..offspring {
            let mut table = vec![[0.0f64; NJ]; markers + 1];
            let mut q = [0.0f64; NJ];
            self.node_costs(model, c, markers - 1, &mut node, &mut arg);
            q.copy_from_slice(&node);
            for m in (1..markers).rev() {
                let ta = &model.ta[m - 1];
                let tb = &model.tb[m - 1];
                let row = &mut table[m];
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
                    self.node_costs(model, c, m - 1, &mut node, &mut arg);
                    for j in 0..NJ {
                        q[j] = node[j] + table[m][j];
                    }
                }
            }
            // Index 0 is the bound from the start: Q at marker 0.
            self.node_costs(model, c, 0, &mut node, &mut arg);
            for j in 0..NJ {
                table[0][j] = node[j] + table[1][j];
            }
            s.push(table);
        }

        let mut csuf = vec![0.0f64; markers + 1];
        let mut run = 0.0f64;
        for m in (0..markers).rev() {
            run += self.cheapest(model, m);
            csuf[m] = run;
        }
        Suffix { s, csuf }
    }
}

/// The Lagrangian bound as a suffix table.
pub struct Suffix {
    /// `s[c][m][j]`: most progeny `c` can still add from marker `m` on, given
    /// state `j` at marker `m - 1`. Index `markers` is all zero.
    pub s: Vec<Vec<[f64; NJ]>>,
    /// `csuf[m]`: `sum over m' >= m` of the consensus correction.
    pub csuf: Vec<f64>,
}

impl Suffix {
    /// Bound on the total, given every progeny's forward vector at `depth` and
    /// the first unassigned marker `m = depth + 1`.
    pub fn bound(&self, m: usize, fwd: &[[f64; NJ]]) -> f64 {
        let mut total = 0.0f64;
        for c in 0..fwd.len() {
            let f = &fwd[c];
            let r = &self.s[c][m];
            let mut best = NEG;
            for j in 0..NJ {
                let v = f[j] + r[j];
                if v > best {
                    best = v;
                }
            }
            total += best;
        }
        total - self.csuf[m]
    }
}
