
//! Canonical form, and the order among answers that score the same.
//!
//! Homolog labels are arbitrary, so `spec/result-format-v1.md` fixes one
//! relabelling of every answer and accepts only that one. Ties survive
//! canonicalisation, so the same document also fixes which of several equally
//! scoring answers is required: smallest parent A rows, then parent B rows,
//! then the path signature.
//!
//! Picking paths is what makes it delicate. When several paths score the same
//! for a progeny, the required one is the smallest in signature order, and that
//! order runs over the whole marker sequence: taking the smaller state at one
//! marker can force a larger one later, so the choice cannot be made one marker
//! at a time. `minimal_paths` settles it with a prefix and a suffix table per
//! progeny, walking markers forwards and taking the smallest state that still
//! leaves the optimum reachable.

use crate::cross_model::{
    Haplotypes, Parent, ProgenyPath, Reconstruction, State, ALL_STATES, HOMOLOGS,
};
use crate::solver::model::{Model, NEG, NJ, NS};

/// A reconstruction in plain indices, before it becomes a `Reconstruction`.
///
/// `rows[parent][homolog][marker]` and `states[parent][progeny][marker]`.
pub struct Raw {
    pub rows: [Vec<Vec<u8>>; 2],
    pub states: [Vec<Vec<usize>>; 2],
}

impl Raw {
    pub fn to_reconstruction(&self) -> Reconstruction {
        let build = |parent: usize| -> Haplotypes {
            let mut haps = Haplotypes::zeros(self.rows[parent][0].len());
            for h in 0..HOMOLOGS {
                for (m, value) in self.rows[parent][h].iter().enumerate() {
                    haps.set(h, m, *value);
                }
            }
            haps
        };
        let progeny = self.states[0].len();
        let mut paths = Vec::with_capacity(progeny);
        for c in 0..progeny {
            let from_a = self.states[0][c].iter().map(|s| state_of(*s)).collect();
            let from_b = self.states[1][c].iter().map(|s| state_of(*s)).collect();
            paths.push(ProgenyPath { from_a, from_b });
        }
        Reconstruction {
            haplotypes_a: build(0),
            haplotypes_b: build(1),
            paths,
        }
    }
}

/// Canonical parent state index of the homologs `low` and `high`.
fn state_index(low: u8, high: u8) -> usize {
    ALL_STATES
        .iter()
        .position(|s| s.low() == low && s.high() == high)
        .expect("every homolog pair is a canonical state")
}

fn state_of(index: usize) -> State {
    ALL_STATES[index]
}

/// Lower and upper homolog of each canonical parent state.
fn low_of(index: usize) -> u8 {
    ALL_STATES[index].low()
}

fn high_of(index: usize) -> u8 {
    ALL_STATES[index].high()
}

/// All 24 relabellings of four homologs, in ascending image order.
pub fn permutations() -> Vec<[u8; HOMOLOGS]> {
    let mut out = Vec::with_capacity(24);
    for a in 0..HOMOLOGS as u8 {
        for b in 0..HOMOLOGS as u8 {
            for c in 0..HOMOLOGS as u8 {
                for d in 0..HOMOLOGS as u8 {
                    let perm = [a, b, c, d];
                    let mut seen = [false; HOMOLOGS];
                    if perm.iter().all(|x| {
                        let fresh = !seen[*x as usize];
                        seen[*x as usize] = true;
                        fresh
                    }) {
                        out.push(perm);
                    }
                }
            }
        }
    }
    out
}

/// Where a state goes under a relabelling, as a canonical state index.
fn remap(state: usize, perm: &[u8; HOMOLOGS]) -> usize {
    let (a, b) = (perm[usize::from(low_of(state))], perm[usize::from(high_of(state))]);
    if a <= b {
        state_index(a, b)
    } else {
        state_index(b, a)
    }
}

/// What the canonical form minimises for one parent: the reordered rows, then
/// that parent's remapped path signature, then the image tuple.
fn relabel_key(raw: &Raw, parent: usize, perm: &[u8; HOMOLOGS]) -> (Vec<Vec<u8>>, Vec<u8>, [u8; HOMOLOGS]) {
    let mut rows = vec![Vec::new(); HOMOLOGS];
    for h in 0..HOMOLOGS {
        rows[usize::from(perm[h])] = raw.rows[parent][h].clone();
    }
    let mut signature = Vec::new();
    for progeny in raw.states[parent].iter() {
        for state in progeny.iter() {
            let moved = remap(*state, perm);
            signature.push(low_of(moved));
            signature.push(high_of(moved));
        }
    }
    (rows, signature, *perm)
}

/// The canonical relabelling of one parent.
fn chosen_perm(raw: &Raw, parent: usize) -> [u8; HOMOLOGS] {
    let mut best: Option<(Vec<Vec<u8>>, Vec<u8>, [u8; HOMOLOGS])> = None;
    for perm in permutations() {
        let key = relabel_key(raw, parent, &perm);
        if best.as_ref().map_or(true, |b| key < *b) {
            best = Some(key);
        }
    }
    best.expect("there are relabellings").2
}

/// Applies the canonical relabelling to both parents.
pub fn canonicalise(raw: &Raw) -> Raw {
    let mut out_rows: [Vec<Vec<u8>>; 2] = [Vec::new(), Vec::new()];
    let mut out_states: [Vec<Vec<usize>>; 2] = [Vec::new(), Vec::new()];
    for parent in 0..2 {
        let perm = chosen_perm(raw, parent);
        let mut rows = vec![Vec::new(); HOMOLOGS];
        for h in 0..HOMOLOGS {
            rows[usize::from(perm[h])] = raw.rows[parent][h].clone();
        }
        out_rows[parent] = rows;
        let mut per_progeny = Vec::with_capacity(raw.states[parent].len());
        for progeny in raw.states[parent].iter() {
            per_progeny.push(progeny.iter().map(|s| remap(*s, &perm)).collect());
        }
        out_states[parent] = per_progeny;
    }
    Raw { rows: out_rows, states: out_states }
}

/// The order among distinct optima: parent A rows, parent B rows, then every
/// progeny's parent A states followed by its parent B states.
pub fn distinct_key(raw: &Raw) -> Vec<u8> {
    let mut key = Vec::new();
    for parent in 0..2 {
        for row in raw.rows[parent].iter() {
            key.extend_from_slice(row);
        }
    }
    let progeny = raw.states[0].len();
    for c in 0..progeny {
        for parent in 0..2 {
            for state in raw.states[parent][c].iter() {
                key.push(low_of(*state));
                key.push(high_of(*state));
            }
        }
    }
    key
}

/// Relabellings that leave the rows sorted, which are the only ones the
/// canonical form can choose between.
fn sorting_perms(rows: &[Vec<u8>]) -> Vec<[u8; HOMOLOGS]> {
    permutations()
        .into_iter()
        .filter(|perm| {
            let mut moved = vec![Vec::new(); HOMOLOGS];
            for h in 0..HOMOLOGS {
                moved[usize::from(perm[h])] = rows[h].clone();
            }
            moved.windows(2).all(|pair| pair[0] <= pair[1])
        })
        .collect()
}

/// Parent states ordered by their remapped image, so that the smallest one is
/// the first the signature wants.
fn state_order(perm: &[u8; HOMOLOGS]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..NS).collect();
    order.sort_by_key(|s| {
        let (a, b) = (perm[usize::from(low_of(*s))], perm[usize::from(high_of(*s))]);
        (a.min(b), a.max(b))
    });
    order
}

/// The answer for one assignment: canonical, and smallest among the
/// relabellings that the evidence leaves open.
pub fn answer_for(model: &Model, z: &[usize]) -> Reconstruction {
    let mut rows: [Vec<Vec<u8>>; 2] = [Vec::new(), Vec::new()];
    for parent in 0..2 {
        let mut per_homolog = Vec::with_capacity(HOMOLOGS);
        for h in 0..HOMOLOGS {
            let mut row = Vec::with_capacity(model.markers);
            for m in 0..model.markers {
                let (ia, ib) = split_z(model, m, z[m]);
                let column = if parent == 0 {
                    model.columns[0][m][ia]
                } else {
                    model.columns[1][m][ib]
                };
                row.push(column[h]);
            }
            per_homolog.push(row);
        }
        rows[parent] = per_homolog;
    }

    let mut best: Option<(Vec<u8>, Raw)> = None;
    let perms_a = sorting_perms(&rows[0]);
    let perms_b = sorting_perms(&rows[1]);
    for perm_a in perms_a.iter() {
        for perm_b in perms_b.iter() {
            let states = minimal_paths(model, z, perm_a, perm_b);
            let raw = canonicalise(&Raw { rows: rows.clone(), states });
            let key = distinct_key(&raw);
            if best.as_ref().map_or(true, |b| key < b.0) {
                best = Some((key, raw));
            }
        }
    }
    best.expect("there is always a relabelling").1.to_reconstruction()
}

fn split_z(model: &Model, m: usize, z: usize) -> (usize, usize) {
    let nb = model.ncol[1][m];
    (z / nb, z % nb)
}

/// For every progeny the smallest optimal path in signature order.
///
/// Parent A's states are settled first, over the whole marker sequence, and
/// then parent B's, because that is the order the key compares them in.
fn minimal_paths(
    model: &Model,
    z: &[usize],
    perm_a: &[u8; HOMOLOGS],
    perm_b: &[u8; HOMOLOGS],
) -> [Vec<Vec<usize>>; 2] {
    let markers = model.markers;
    let offspring = model.offspring;
    let order_a = state_order(perm_a);
    let order_b = state_order(perm_b);
    let mut out: [Vec<Vec<usize>>; 2] = [Vec::new(), Vec::new()];
    for parent in 0..2 {
        out[parent] = vec![Vec::with_capacity(markers); offspring];
    }
    if markers == 0 {
        return out;
    }

    for c in 0..offspring {
        let mut node: Vec<Vec<f64>> = Vec::with_capacity(markers);
        for m in 0..markers {
            let mut row = vec![0.0f64; NJ];
            for j in 0..NJ {
                row[j] = model.node_cost(c, m, z[m], j);
            }
            node.push(row);
        }
        let optimum = viterbi(model, &node);
        let slack = optimum - 1e-9;

        // Suffix: best value of the markers after `m`, given the state at `m`.
        let mut after: Vec<Vec<f64>> = vec![vec![0.0f64; NJ]; markers];
        for m in (0..markers - 1).rev() {
            let ta = &model.ta[m];
            let tb = &model.tb[m];
            for j in 0..NJ {
                let (a, b) = (j / NS, j % NS);
                let mut best = NEG;
                for ap in 0..NS {
                    let va = ta[a * NS + ap];
                    for bp in 0..NS {
                        let v = va + tb[b * NS + bp] + node[m + 1][ap * NS + bp]
                            + after[m + 1][ap * NS + bp];
                        if v > best {
                            best = v;
                        }
                    }
                }
                after[m][j] = best;
            }
        }

        // Parent A first: pin one state at a time, taking the smallest that
        // still reaches the optimum. The prefix keeps every joint state, since
        // the score couples the two parents at each marker.
        let mut prefix = vec![NEG; NJ];
        let mut chosen_a = Vec::with_capacity(markers);
        for m in 0..markers {
            let mut taken = 0usize;
            let mut taken_prefix = vec![NEG; NJ];
            for state in order_a.iter() {
                let a = *state;
                let mut candidate = vec![NEG; NJ];
                for b in 0..NS {
                    let j = a * NS + b;
                    if node[m][j] == NEG {
                        continue;
                    }
                    let mut best = NEG;
                    if m == 0 {
                        best = node[m][j];
                    } else {
                        let previous = chosen_a[m - 1];
                        let ta = &model.ta[m - 1];
                        let tb = &model.tb[m - 1];
                        for bp in 0..NS {
                            let before = prefix[previous * NS + bp];
                            if before == NEG {
                                continue;
                            }
                            let v = before + ta[previous * NS + a] + tb[bp * NS + b];
                            if v > best {
                                best = v;
                            }
                        }
                        if best == NEG {
                            continue;
                        }
                        best += node[m][j];
                    }
                    candidate[j] = best;
                }
                let mut reachable = NEG;
                for j in 0..NJ {
                    let v = candidate[j] + after[m][j];
                    if v > reachable {
                        reachable = v;
                    }
                }
                if reachable >= slack {
                    taken = a;
                    taken_prefix = candidate;
                    break;
                }
            }
            prefix = taken_prefix;
            chosen_a.push(taken);
        }

        // Then parent B, with the states of A now fixed: a plain chain.
        let node_b: Vec<Vec<f64>> = (0..markers)
            .map(|m| (0..NS).map(|b| node[m][chosen_a[m] * NS + b]).collect())
            .collect();
        let fixed_a: Vec<f64> = (0..markers)
            .map(|m| {
                if m == 0 {
                    0.0
                } else {
                    model.ta[m - 1][chosen_a[m - 1] * NS + chosen_a[m]]
                }
            })
            .collect();
        let mut after_b: Vec<Vec<f64>> = vec![vec![0.0f64; NS]; markers];
        for m in (0..markers - 1).rev() {
            let tb = &model.tb[m];
            for b in 0..NS {
                let mut best = NEG;
                for bp in 0..NS {
                    let v = tb[b * NS + bp] + node_b[m + 1][bp] + fixed_a[m + 1] + after_b[m + 1][bp];
                    if v > best {
                        best = v;
                    }
                }
                after_b[m][b] = best;
            }
        }
        let mut prefix_b = vec![NEG; NS];
        let mut chosen_b = Vec::with_capacity(markers);
        for m in 0..markers {
            let mut taken = 0usize;
            let mut taken_prefix = vec![NEG; NS];
            for state in order_b.iter() {
                let b = *state;
                if node_b[m][b] == NEG {
                    continue;
                }
                let mut best = NEG;
                if m == 0 {
                    best = node_b[m][b];
                } else {
                    let tb = &model.tb[m - 1];
                    for bp in 0..NS {
                        let before = prefix_b[bp];
                        if before == NEG {
                            continue;
                        }
                        let v = before + tb[bp * NS + b];
                        if v > best {
                            best = v;
                        }
                    }
                    if best == NEG {
                        continue;
                    }
                    best += node_b[m][b] + fixed_a[m];
                }
                if best + after_b[m][b] >= slack {
                    taken = b;
                    let mut candidate = vec![NEG; NS];
                    candidate[b] = best;
                    taken_prefix = candidate;
                    break;
                }
            }
            prefix_b = taken_prefix;
            chosen_b.push(taken);
        }

        out[0][c] = chosen_a;
        out[1][c] = chosen_b;
    }
    out
}

/// Best score for one progeny over its node costs.
fn viterbi(model: &Model, node: &[Vec<f64>]) -> f64 {
    let markers = node.len();
    let mut cur = node[0].clone();
    let mut next = vec![0.0f64; NJ];
    for m in 1..markers {
        let ta = &model.ta[m - 1];
        let tb = &model.tb[m - 1];
        for ap in 0..NS {
            let row = &ta[ap * NS..ap * NS + NS];
            for bp in 0..NS {
                let mut best = NEG;
                for a in 0..NS {
                    for b in 0..NS {
                        let v = cur[a * NS + b] + row[a] + tb[b * NS + bp];
                        if v > best {
                            best = v;
                        }
                    }
                }
                next[ap * NS + bp] = best + node[m][ap * NS + bp];
            }
        }
        cur.copy_from_slice(&next);
    }
    let mut best = NEG;
    for v in cur.iter() {
        if *v > best {
            best = *v;
        }
    }
    best
}

/// Unused: kept so the parent enum stays referenced from this module.
pub fn parents() -> [Parent; 2] {
    Parent::BOTH
}
