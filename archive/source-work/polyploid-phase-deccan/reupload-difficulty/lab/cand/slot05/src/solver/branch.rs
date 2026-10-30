//! The search itself: an alternating optimisation that supplies a lower bound,
//! and a branch and bound that turns it into a proof.
//!
//! ## What is being searched
//!
//! A parental assignment is one admissible allele column per marker per parent.
//! Progeny paths are then independent, and each is a Viterbi problem over the
//! hundred joint states. So the objective is a function of the assignment
//! alone, and the search enumerates assignments.
//!
//! ## Symmetry
//!
//! Homolog labels are arbitrary, so the `4!` relabellings of a parent all give
//! the same score. The search walks one representative per orbit: at each
//! marker it offers only those columns that are minimal in their orbit under
//! the permutations that still fix every column chosen so far. That is the
//! standard canonical construction path argument — a sequence whose every
//! prefix is orbit minimal is the least element of its orbit, and the least
//! element of each orbit is reached exactly once.
//!
//! ## The bound
//!
//! At a node whose columns are fixed up to `marker`, each progeny carries a
//! forward vector over joint states: the best score of markers `0..marker`
//! ending in that state. A precomputed backward table holds, for every progeny,
//! marker and state, the best continuation *if every later marker were free to
//! take its most favourable admissible column*. Adding the two and maximising
//! over states bounds what any completion of this prefix can reach, so a node
//! whose bound falls below the best known assignment is dropped.

use crate::solver::dual::Dual;
use crate::solver::model::{Model, NJ, NS};

/// Slack allowed when pruning. Wider than the `1e-8` that counts two totals as
/// equal, so that a node holding a tying solution is never dropped, and much
/// wider than the floating point drift in the bound.
const PRUNE_EPS: f64 = 1e-7;
/// Two totals within this much of the best are both optimal.
pub const TIE: f64 = 1e-8;

const NEG: f64 = f64::NEG_INFINITY;

/// Best path through one progeny's node rows, with the states that attain it.
///
/// `nodes` is indexed `[marker][joint]`. Returns the value and the joint state
/// at each marker.
pub fn viterbi(model: &Model, nodes: &[[f64; NJ]]) -> (f64, Vec<usize>) {
    let m = model.markers;
    let mut f = [NEG; NJ];
    for j in 0..NJ {
        f[j] = nodes[0][j];
    }
    let mut back: Vec<[usize; NJ]> = Vec::with_capacity(m);
    back.push([0usize; NJ]);
    for mm in 1..m {
        // The transition term factorises over the two parents, so the hundred
        // by hundred step is taken as two ten by ten steps.
        let mut mid = [NEG; NJ];
        let mut mid_arg = [[0usize; NS]; NS];
        let ta = &model.ta[mm - 1];
        for sb in 0..NS {
            for sat in 0..NS {
                let mut best = NEG;
                let mut arg = 0usize;
                for sa in 0..NS {
                    let v = f[sa * NS + sb] + ta[sa][sat];
                    if v > best {
                        best = v;
                        arg = sa;
                    }
                }
                mid[sat * NS + sb] = best;
                mid_arg[sat][sb] = arg;
            }
        }
        let tb = &model.tb[mm - 1];
        let mut nf = [NEG; NJ];
        let mut bp = [0usize; NJ];
        for sat in 0..NS {
            for sbt in 0..NS {
                let j = sat * NS + sbt;
                if model.legal[mm][j] {
                    let mut best = NEG;
                    let mut arg = 0usize;
                    for sb in 0..NS {
                        let v = mid[sat * NS + sb] + tb[sb][sbt];
                        if v > best {
                            best = v;
                            arg = sb;
                        }
                    }
                    nf[j] = best + nodes[mm][j];
                    bp[j] = mid_arg[sat][arg] * NS + arg;
                }
            }
        }
        back.push(bp);
        f = nf;
    }
    let mut best = NEG;
    let mut arg = 0usize;
    for j in 0..NJ {
        if f[j] > best {
            best = f[j];
            arg = j;
        }
    }
    let mut path = vec![0usize; m];
    let mut cur = arg;
    for mm in (0..m).rev() {
        path[mm] = cur;
        if mm > 0 {
            cur = back[mm][cur];
        }
    }
    (best, path)
}

/// Best total for one progeny under a fixed assignment.
pub fn progeny_value(model: &Model, progeny: usize, assign: &[(usize, usize)]) -> f64 {
    let mut nodes: Vec<[f64; NJ]> = Vec::with_capacity(model.markers);
    model.nodes_for(progeny, assign, &mut nodes);
    viterbi(model, &nodes).0
}

/// Total of an assignment: progeny are independent once the parents are fixed.
pub fn assignment_value(model: &Model, assign: &[(usize, usize)]) -> f64 {
    let mut nodes: Vec<[f64; NJ]> = Vec::with_capacity(model.markers);
    let mut total = 0.0f64;
    for c in 0..model.offspring {
        model.nodes_for(c, assign, &mut nodes);
        total += viterbi(model, &nodes).0;
    }
    total
}

/// Alternating optimisation from one start: its value and where it settles.
///
/// Fix the assignment and every progeny takes its best path; fix the paths and
/// every marker takes its best column pair — the double reduction and
/// transition terms do not involve the parental alleles at all, so that second
/// step is a small independent maximisation per marker. Each round can only
/// improve the total, and it stops once it stops moving.
pub fn polish(
    model: &Model,
    start: &[(usize, usize)],
    rounds: usize,
) -> (f64, Vec<(usize, usize)>) {
    let mut assign = start.to_vec();
    let mut best = NEG;
    let mut best_assign = assign.clone();
    let mut nodes: Vec<[f64; NJ]> = Vec::with_capacity(model.markers);
    let mut previous = NEG;
    for _ in 0..rounds {
        let mut total = 0.0f64;
        let mut paths: Vec<Vec<usize>> = Vec::with_capacity(model.offspring);
        for c in 0..model.offspring {
            model.nodes_for(c, &assign, &mut nodes);
            let (v, path) = viterbi(model, &nodes);
            total += v;
            paths.push(path);
        }
        if total > best {
            best = total;
            best_assign = assign.clone();
        }
        if (total - previous).abs() < 1e-9 {
            break;
        }
        previous = total;
        for m in 0..model.markers {
            let mut best_pair = (0usize, 0usize);
            let mut best_value = NEG;
            for ia in 0..model.na[m] {
                for ib in 0..model.nb[m] {
                    let dose = &model.dose[m][model.pair(m, ia, ib)];
                    let mut sum = 0.0f64;
                    for c in 0..model.offspring {
                        sum += model.gl[c][m][dose[paths[c][m]] as usize];
                    }
                    if sum > best_value {
                        best_value = sum;
                        best_pair = (ia, ib);
                    }
                }
            }
            assign[m] = best_pair;
        }
    }
    (best, best_assign)
}

/// Alternating optimisation from random starts, keeping the best it reaches.
///
/// This supplies the assignment the search prunes against; it does not have to
/// be optimal, only good.
pub fn local_search(model: &Model, restarts: u64) -> (f64, Vec<(usize, usize)>) {
    let mut rng = 0x9E3779B97F4A7C15u64;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };

    let mut best = NEG;
    let mut best_assign: Vec<(usize, usize)> = (0..model.markers).map(|_| (0usize, 0usize)).collect();
    for _ in 0..restarts {
        let start: Vec<(usize, usize)> = (0..model.markers)
            .map(|m| ((next() % model.na[m] as u64) as usize, (next() % model.nb[m] as u64) as usize))
            .collect();
        let (value, assign) = polish(model, &start, 24);
        if value > best {
            best = value;
            best_assign = assign;
        }
    }
    (best, best_assign)
}

pub struct Outcome {
    pub best: f64,
    /// Bound at the root: what the continuation table allows with nothing fixed.
    #[allow(dead_code)]
    pub root_bound: f64,
    /// Every assignment whose total ties with `best`, column choices per marker.
    pub assignments: Vec<Vec<(usize, usize)>>,
    #[allow(dead_code)]
    pub nodes: u64,
    #[allow(dead_code)]
    pub leaves: u64,
}

/// Proves the lower bound optimal, or replaces it with a better one.
///
/// Returns the optimum and every assignment that attains it, so that the
/// representative demanded by `spec/result-format-v1.md` can be chosen among
/// them afterwards.
pub fn branch_and_bound(model: &Model, lower_bound: f64, dual: &Dual) -> Outcome {
    let n = model.offspring;
    let m = model.markers;

    // Continuation bound: from joint state `j` at `marker`, through the rest.
    let mut back: Vec<Vec<[f64; NJ]>> = Vec::with_capacity(n);
    for c in 0..n {
        let mut rows = vec![[0.0f64; NJ]; m];
        for mm in (0..m - 1).rev() {
            let mut r = [NEG; NJ];
            for sat in 0..NS {
                for sbt in 0..NS {
                    let j = sat * NS + sbt;
                    if !model.legal[mm + 1][j] {
                        continue;
                    }
                    let onward = dual.nu(c, mm + 1)[j] + rows[mm + 1][j];
                    for sa in 0..NS {
                        for sb in 0..NS {
                            let jj = sa * NS + sb;
                            let v = model.ta[mm][sa][sat] + model.tb[mm][sb][sbt] + onward;
                            if v > r[jj] {
                                r[jj] = v;
                            }
                        }
                    }
                }
            }
            rows[mm] = r;
        }
        back.push(rows);
    }

    // Every term is a log likelihood, so none is positive: any partial sum of
    // progeny bounds only falls. Progeny that fall furthest come first, which
    // makes a hopeless node fail its bound after a few of them.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&x, &y| {
        let sx: f64 = (0..m).map(|mm| model.gl[x][mm].iter().cloned().fold(NEG, f64::max)).sum();
        let sy: f64 = (0..m).map(|mm| model.gl[y][mm].iter().cloned().fold(NEG, f64::max)).sum();
        sx.partial_cmp(&sy).unwrap()
    });

    let root_bound: f64 = (0..n)
        .map(|c| {
            let mut best = NEG;
            for j in 0..NJ {
                let v = dual.nu(c, 0)[j] + back[c][0][j];
                if v > best {
                    best = v;
                }
            }
            best
        })
        .sum::<f64>()
        - dual.suffix(0);
    let mut state = Bnb {
        model,
        dual,
        back,
        fwd: vec![vec![[NEG; NJ]; n]; m],
        best: lower_bound,
        ties: Vec::new(),
        stack: Vec::with_capacity(m),
        order,
        nodes: 0,
        leaves: 0,
    };
    let all = crate::solver::canon::perms4();
    state.dfs(0, &all, &all);

    let best = state.best;
    let mut assignments: Vec<Vec<(usize, usize)>> = state
        .ties
        .into_iter()
        .filter(|(value, _)| *value >= best - TIE)
        .map(|(_, assign)| assign)
        .collect();
    if assignments.is_empty() {
        // Only reachable if the lower bound already exceeded every leaf, which
        // happens when the bound is the optimum found by local search and the
        // search kept nothing: recover it from the bound.
        assignments = Vec::new();
    }
    Outcome { best, root_bound, assignments, nodes: state.nodes, leaves: state.leaves }
}

struct Bnb<'a> {
    model: &'a Model,
    dual: &'a Dual,
    back: Vec<Vec<[f64; NJ]>>,
    /// `[marker][progeny][joint]` best score of the fixed prefix ending there.
    fwd: Vec<Vec<[f64; NJ]>>,
    best: f64,
    ties: Vec<(f64, Vec<(usize, usize)>)>,
    stack: Vec<(usize, usize)>,
    order: Vec<usize>,
    nodes: u64,
    leaves: u64,
}

impl<'a> Bnb<'a> {
    fn dfs(&mut self, marker: usize, ga: &[[u8; 4]], gb: &[[u8; 4]]) {
        let model = self.model;
        self.nodes += 1;
        if marker == model.markers {
            self.leaves += 1;
            let mut total = 0.0f64;
            for c in 0..model.offspring {
                let f = &self.fwd[marker - 1][c];
                let mut best = NEG;
                for j in 0..NJ {
                    if f[j] > best {
                        best = f[j];
                    }
                }
                total += best;
            }
            if total > self.best + TIE {
                self.best = total;
                self.ties.clear();
                self.ties.push((total, self.stack.clone()));
            } else if total >= self.best - TIE {
                self.ties.push((total, self.stack.clone()));
            }
            return;
        }

        let candidates_a = orbit_representatives(&model.cols_a[marker], ga);
        let candidates_b = orbit_representatives(&model.cols_b[marker], gb);

        for ia in &candidates_a {
            let column_a = model.cols_a[marker][*ia];
            let next_ga = stabiliser(ga, &column_a);
            for ib in &candidates_b {
                let column_b = model.cols_b[marker][*ib];
                let next_gb = stabiliser(gb, &column_b);
                let pair = model.pair(marker, *ia, *ib);
                let dose = &model.dose[marker][pair];
                // What the multipliers charge the copies for the markers still
                // free, credited back because the progeny below share one column.
                let mut bound = -self.dual.suffix(marker);
                let mut alive = true;
                for &c in &self.order {
                    let g = &model.gl[c][marker];
                    let dr = &model.drj[marker];
                    let mut nf = [NEG; NJ];
                    if marker == 0 {
                        for j in 0..NJ {
                            if model.legal[0][j] {
                                nf[j] = g[dose[j] as usize] + dr[j];
                            }
                        }
                    } else {
                        let prev = self.fwd[marker - 1][c];
                        let mut mid = [NEG; NJ];
                        let ta = &model.ta[marker - 1];
                        for sb in 0..NS {
                            for sat in 0..NS {
                                let mut best = NEG;
                                for sa in 0..NS {
                                    let v = prev[sa * NS + sb] + ta[sa][sat];
                                    if v > best {
                                        best = v;
                                    }
                                }
                                mid[sat * NS + sb] = best;
                            }
                        }
                        let tb = &model.tb[marker - 1];
                        for sat in 0..NS {
                            for sbt in 0..NS {
                                let j = sat * NS + sbt;
                                if !model.legal[marker][j] {
                                    continue;
                                }
                                let mut best = NEG;
                                for sb in 0..NS {
                                    let v = mid[sat * NS + sb] + tb[sb][sbt];
                                    if v > best {
                                        best = v;
                                    }
                                }
                                nf[j] = best + g[dose[j] as usize] + dr[j];
                            }
                        }
                    }
                    self.fwd[marker][c] = nf;
                    let bk = &self.back[c][marker];
                    let mut best = NEG;
                    for j in 0..NJ {
                        let v = nf[j] + bk[j];
                        if v > best {
                            best = v;
                        }
                    }
                    bound += best;
                    if bound < self.best - PRUNE_EPS {
                        alive = false;
                        break;
                    }
                }
                if alive {
                    self.stack.push((*ia, *ib));
                    self.dfs(marker + 1, &next_ga, &next_gb);
                    self.stack.pop();
                }
            }
        }
    }
}

/// Columns of `marker` that are least in their orbit under `group`.
fn orbit_representatives(columns: &[[u8; 4]], group: &[[u8; 4]]) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, column) in columns.iter().enumerate() {
        let least = group
            .iter()
            .map(|p| crate::solver::canon::apply_perm(p, column))
            .all(|image| crate::solver::canon::column_key(&image) >= crate::solver::canon::column_key(column));
        if least {
            out.push(i);
        }
    }
    out
}

/// The subgroup of `group` that leaves `column` untouched.
fn stabiliser(group: &[[u8; 4]], column: &[u8; 4]) -> Vec<[u8; 4]> {
    group
        .iter()
        .filter(|p| crate::solver::canon::apply_perm(p, column) == *column)
        .cloned()
        .collect()
}
