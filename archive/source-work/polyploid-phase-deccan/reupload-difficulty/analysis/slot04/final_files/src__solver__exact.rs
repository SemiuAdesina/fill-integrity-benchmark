
//! Branch and bound over the column sequence.
//!
//! Markers are fixed in order and every node carries, for each progeny, the
//! Viterbi forward value of every joint state after the markers decided so far.
//! Two bounds prune a node: one keeps the paths consistent and lets each
//! progeny imagine its own columns for the markers left, the other keeps the
//! columns shared and lets the states jump.

use crate::solver::core::{Tables, NJ, NS};

/// Best possible transition term between two markers, per progeny.
fn transition_ceiling(t: &Tables, interval: usize) -> f64 {
    t.trans[0][interval][0] + t.trans[1][interval][0]
}

/// One complete assignment reaching the best score seen.
#[derive(Clone)]
pub struct Candidate {
    pub value: f64,
    pub columns_a: Vec<usize>,
    pub columns_b: Vec<usize>,
}

pub struct Branch<'a> {
    t: &'a Tables,
    /// `suffix[c][m][j]`: best score from marker `m` to the end, starting in
    /// state `j`, using optimistic emissions.
    suffix: Vec<Vec<[f64; NJ]>>,
    /// `reach[c][m]`: best of `transition + suffix` over both endpoints, used
    /// for a cheap child bound.
    reach: Vec<Vec<f64>>,
    /// Per marker bound with the columns shared and the states free.
    per_marker_tail: Vec<f64>,
    /// Forward vectors, one level per marker.
    levels: Vec<Vec<[f64; NJ]>>,
    /// Column choices along the current path.
    choice_a: Vec<usize>,
    choice_b: Vec<usize>,
    /// Solutions within `slack` of the best score.
    pub ties: Vec<Candidate>,
    pub best_value: f64,
    pub best_a: Vec<usize>,
    pub best_b: Vec<usize>,
    pub slack: f64,
    pub nodes: u64,
    pub pruned: u64,
}

impl<'a> Branch<'a> {
    pub fn new(
        t: &'a Tables,
        lower_bound: f64,
        columns_a: &[usize],
        columns_b: &[usize],
    ) -> Branch<'a> {
        let markers = t.markers;
        let offspring = t.offspring;

        let optimistic = crate::solver::bound::optimistic_emissions(t);

        let mut suffix = vec![vec![[f64::NEG_INFINITY; NJ]; markers]; offspring];
        for c in 0..offspring {
            for j in 0..NJ {
                if t.legalj[markers - 1][j] {
                    suffix[c][markers - 1][j] =
                        optimistic[c][markers - 1][j] + t.drj[markers - 1][j];
                }
            }
            for m in (0..markers - 1).rev() {
                let ta = &t.trans[0][m];
                let tb = &t.trans[1][m];
                let mut x = [f64::NEG_INFINITY; NJ];
                for pa in 0..NS {
                    for sb in 0..NS {
                        let mut best = f64::NEG_INFINITY;
                        for pb in 0..NS {
                            let value = suffix[c][m + 1][pa * NS + pb] + tb[pb * NS + sb];
                            if value > best {
                                best = value;
                            }
                        }
                        x[pa * NS + sb] = best;
                    }
                }
                for sa in 0..NS {
                    for sb in 0..NS {
                        let j = sa * NS + sb;
                        if !t.legalj[m][j] {
                            continue;
                        }
                        let mut best = f64::NEG_INFINITY;
                        for pa in 0..NS {
                            let value = x[pa * NS + sb] + ta[pa * NS + sa];
                            if value > best {
                                best = value;
                            }
                        }
                        suffix[c][m][j] = best + optimistic[c][m][j] + t.drj[m][j];
                    }
                }
            }
        }

        let mut reach = vec![vec![0.0f64; markers]; offspring];
        for c in 0..offspring {
            for m in 0..markers - 1 {
                let ta = &t.trans[0][m];
                let tb = &t.trans[1][m];
                let mut best = f64::NEG_INFINITY;
                for j in 0..NJ {
                    if suffix[c][m + 1][j] == f64::NEG_INFINITY {
                        continue;
                    }
                    for k in 0..NJ {
                        let value = suffix[c][m + 1][k]
                            + ta[(j / NS) * NS + k / NS]
                            + tb[(j % NS) * NS + k % NS];
                        if value > best {
                            best = value;
                        }
                    }
                }
                reach[c][m] = best;
            }
        }

        let mut per_marker_tail = vec![0.0f64; markers + 1];
        for m in (0..markers).rev() {
            let na = t.contrib[0][m].len();
            let nb = t.contrib[1][m].len();
            let mut best = f64::NEG_INFINITY;
            for oa in 0..na {
                let ea = &t.contrib[0][m][oa];
                for ob in 0..nb {
                    let eb = &t.contrib[1][m][ob];
                    let mut sum = 0.0f64;
                    for c in 0..offspring {
                        let mut best_state = f64::NEG_INFINITY;
                        for j in 0..NJ {
                            if !t.legalj[m][j] {
                                continue;
                            }
                            let dosage = ea[j / NS] + eb[j % NS];
                            let value = t.gl[c][m][usize::from(dosage)] + t.drj[m][j];
                            if value > best_state {
                                best_state = value;
                            }
                        }
                        sum += best_state;
                    }
                    if sum > best {
                        best = sum;
                    }
                }
            }
            let mut value = best + per_marker_tail[m + 1];
            if m < markers - 1 {
                value += transition_ceiling(t, m) * offspring as f64;
            }
            per_marker_tail[m] = value;
        }

        Branch {
            t,
            suffix,
            reach,
            per_marker_tail,
            levels: vec![vec![[f64::NEG_INFINITY; NJ]; offspring]; markers],
            choice_a: columns_a.to_vec(),
            choice_b: columns_b.to_vec(),
            ties: Vec::new(),
            best_value: lower_bound,
            best_a: columns_a.to_vec(),
            best_b: columns_b.to_vec(),
            slack: 1e-9,
            nodes: 0,
            pruned: 0,
        }
    }

    /// Explores the tree. Returns false if it ran out of budget.
    pub fn run(&mut self, budget: u64) -> bool {
        self.descend(0, budget)
    }

    fn descend(&mut self, depth: usize, budget: u64) -> bool {
        if self.nodes > budget {
            return false;
        }
        self.nodes += 1;
        let t = self.t;
        let markers = t.markers;
        let offspring = t.offspring;

        if depth == markers {
            let mut total = 0.0f64;
            let last = &self.levels[markers - 1];
            for c in 0..offspring {
                let mut best = f64::NEG_INFINITY;
                for j in 0..NJ {
                    if last[c][j] > best {
                        best = last[c][j];
                    }
                }
                total += best;
            }
            self.record(total);
            return true;
        }

        // Advance the forward vectors across the interval into `depth`.
        self.levels[depth] = if depth == 0 {
            let mut moved = vec![[f64::NEG_INFINITY; NJ]; offspring];
            for c in 0..offspring {
                for j in 0..NJ {
                    if t.legalj[0][j] {
                        moved[c][j] = 0.0;
                    }
                }
            }
            moved
        } else {
            let previous = self.levels[depth - 1].clone();
            let ta = &t.trans[0][depth - 1];
            let tb = &t.trans[1][depth - 1];
            let mut moved = vec![[f64::NEG_INFINITY; NJ]; offspring];
            for c in 0..offspring {
                let mut x = [f64::NEG_INFINITY; NJ];
                for pa in 0..NS {
                    for sb in 0..NS {
                        let mut best = f64::NEG_INFINITY;
                        for pb in 0..NS {
                            let value = previous[c][pa * NS + pb] + tb[pb * NS + sb];
                            if value > best {
                                best = value;
                            }
                        }
                        x[pa * NS + sb] = best;
                    }
                }
                let mut row = [f64::NEG_INFINITY; NJ];
                for sa in 0..NS {
                    for sb in 0..NS {
                        let j = sa * NS + sb;
                        if !t.legalj[depth][j] {
                            continue;
                        }
                        let mut best = f64::NEG_INFINITY;
                        for pa in 0..NS {
                            let value = x[pa * NS + sb] + ta[pa * NS + sa];
                            if value > best {
                                best = value;
                            }
                        }
                        row[j] = best;
                    }
                }
                moved[c] = row;
            }
            moved
        };

        // Bound one: paths stay consistent, columns are per progeny.
        let mut bound_paths = 0.0f64;
        for c in 0..offspring {
            let mut best = f64::NEG_INFINITY;
            let row = &self.levels[depth][c];
            for j in 0..NJ {
                let value = row[j] + self.suffix[c][depth][j];
                if value > best {
                    best = value;
                }
            }
            bound_paths += best;
        }
        // Bound two: columns stay shared, the states are free per marker.
        let mut bound_markers = self.per_marker_tail[depth];
        if depth > 0 {
            let mut carried = 0.0f64;
            for c in 0..offspring {
                let mut best = f64::NEG_INFINITY;
                for j in 0..NJ {
                    if self.levels[depth - 1][c][j] > best {
                        best = self.levels[depth - 1][c][j];
                    }
                }
                carried += best;
            }
            bound_markers += carried + transition_ceiling(t, depth - 1) * offspring as f64;
        }

        if bound_paths.min(bound_markers) <= self.best_value - self.slack {
            self.pruned += 1;
            return true;
        }

        let na = t.contrib[0][depth].len();
        let nb = t.contrib[1][depth].len();
        let mut options: Vec<(usize, usize, f64)> = Vec::with_capacity(na * nb);
        for oa in 0..na {
            let ea = &t.contrib[0][depth][oa];
            for ob in 0..nb {
                let eb = &t.contrib[1][depth][ob];
                let mut total = 0.0f64;
                for c in 0..offspring {
                    let mut best = f64::NEG_INFINITY;
                    let gl = &t.gl[c][depth];
                    let row = &self.levels[depth][c];
                    for j in 0..NJ {
                        if row[j] == f64::NEG_INFINITY {
                            continue;
                        }
                        let dosage = ea[j / NS] + eb[j % NS];
                        let value = row[j] + gl[usize::from(dosage)] + t.drj[depth][j];
                        if value > best {
                            best = value;
                        }
                    }
                    total += best + self.reach[c][depth];
                }
                options.push((oa, ob, total));
            }
        }
        options.sort_by(|left, right| {
            right.2.partial_cmp(&left.2).unwrap_or(std::cmp::Ordering::Equal)
        });

        for (oa, ob, lookahead) in options {
            if lookahead <= self.best_value - self.slack {
                continue;
            }
            let ea = &t.contrib[0][depth][oa];
            let eb = &t.contrib[1][depth][ob];
            let mut advanced = self.levels[depth].clone();
            for c in 0..offspring {
                let gl = &t.gl[c][depth];
                for j in 0..NJ {
                    if advanced[c][j] == f64::NEG_INFINITY {
                        continue;
                    }
                    let dosage = ea[j / NS] + eb[j % NS];
                    advanced[c][j] += gl[usize::from(dosage)] + t.drj[depth][j];
                }
            }
            self.levels[depth] = advanced;
            self.choice_a[depth] = oa;
            self.choice_b[depth] = ob;
            if !self.descend(depth + 1, budget) {
                return false;
            }
        }
        true
    }

    fn record(&mut self, total: f64) {
        if total > self.best_value + self.slack {
            self.best_value = total;
            self.best_a = self.choice_a.clone();
            self.best_b = self.choice_b.clone();
            self.ties.clear();
            self.ties.push(Candidate {
                value: total,
                columns_a: self.choice_a.clone(),
                columns_b: self.choice_b.clone(),
            });
        } else if total >= self.best_value - self.slack {
            self.ties.push(Candidate {
                value: total,
                columns_a: self.choice_a.clone(),
                columns_b: self.choice_b.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cross_input::parse_cross;
    use crate::solver::core::Tables;
    use crate::solver::search;

    #[test]
    fn branch_and_bound_proves_the_real_cross_optimal() {
        let text = std::fs::read_to_string("/app/cross.json").expect("cross.json");
        let cross = parse_cross(&text).expect("valid cross");
        let t = Tables::build(&cross);
        let (value, xa, xb) = search::search(&t, 24, 24, 0x9e3779b97f4a7c15);
        let mut branch = Branch::new(&t, value - 1e-9, &xa, &xb);
        assert!(branch.run(3_000_000), "the branch and bound ran out of budget");
        assert!(
            branch.best_value >= value - 1e-9,
            "the branch and bound reports {:.6}, below the {value:.6} it started from",
            branch.best_value
        );
        // The assignment it proves optimal has to score what it claims.
        let again = t.evaluate(&branch.best_a, &branch.best_b);
        assert!(
            (again - branch.best_value).abs() < 1e-6,
            "the proven assignment scores {again:.6}, not {:.6}",
            branch.best_value
        );
    }
}
