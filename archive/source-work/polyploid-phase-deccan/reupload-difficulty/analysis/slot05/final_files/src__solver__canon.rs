//! Canonical form, and the choice among reconstructions that tie.
//!
//! `spec/result-format-v1.md` fixes both. Homolog labels are arbitrary, so a
//! relabelling that sorts a parent's rows and rewrites that parent's states is
//! the same answer; the canonical form is the relabelling whose rows ascend,
//! and among those the one whose path signature is least. Two reconstructions
//! that are genuinely different can still score the same, and then the smaller
//! one under a fixed key is required: parent A's rows, then parent B's, then
//! the interleaved path signature.

use crate::cross_model::{Haplotypes, Parent, ProgenyPath, Reconstruction, State, ALL_STATES, HOMOLOGS};
use crate::solver::model::{Model, NJ, NS};

/// Every permutation of the four homolog indices.
pub fn perms4() -> Vec<[u8; HOMOLOGS]> {
    let mut out = Vec::with_capacity(24);
    for a in 0..HOMOLOGS as u8 {
        for b in 0..HOMOLOGS as u8 {
            for c in 0..HOMOLOGS as u8 {
                for d in 0..HOMOLOGS as u8 {
                    let p = [a, b, c, d];
                    let mut seen = [false; HOMOLOGS];
                    if p.iter().all(|x| !std::mem::replace(&mut seen[*x as usize], true)) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

/// Moves the entry at `h` to `perm[h]`.
pub fn apply_perm(perm: &[u8; HOMOLOGS], column: &[u8; HOMOLOGS]) -> [u8; HOMOLOGS] {
    let mut out = [0u8; HOMOLOGS];
    for h in 0..HOMOLOGS {
        out[perm[h] as usize] = column[h];
    }
    out
}

/// Ordering key for allele columns, so columns can be compared as integers.
pub fn column_key(column: &[u8; HOMOLOGS]) -> u8 {
    column[0] | (column[1] << 1) | (column[2] << 2) | (column[3] << 3)
}

/// The ten states ordered by where `perm` sends them.
///
/// Used whenever states have to be compared after relabelling: the tie break
/// asks for the least path signature, and the signature is written in
/// relabelled terms.
fn state_order(perm: &[u8; HOMOLOGS]) -> [usize; NS] {
    let mut order: Vec<usize> = (0..NS).collect();
    order.sort_by_key(|&i| ALL_STATES[i].remap(perm).canonical_index());
    let mut out = [0usize; NS];
    for (slot, state) in order.iter().enumerate() {
        out[slot] = *state;
    }
    out
}

fn rows_ascend(rows: &[Vec<u8>]) -> bool {
    rows.windows(2).all(|w| w[0] <= w[1])
}

/// Path signature of one parent under `perm`: progeny in index order, markers
/// in order, the relabelled `a` then the relabelled `b` of each state.
fn signature(answer: &Reconstruction, parent: Parent, perm: &[u8; HOMOLOGS]) -> Vec<u8> {
    let mut out = Vec::with_capacity(answer.paths.len() * answer.haplotypes(parent).markers() * 2);
    for path in &answer.paths {
        for state in path.states(parent) {
            let moved = state.remap(perm);
            out.push(moved.low());
            out.push(moved.high());
        }
    }
    out
}

/// Permutations that leave this parent's rows in ascending order.
pub fn sorting_permutations(answer: &Reconstruction, parent: Parent) -> Vec<[u8; HOMOLOGS]> {
    let haps = answer.haplotypes(parent);
    let mut out = Vec::new();
    for perm in perms4() {
        let mut rows: Vec<Vec<u8>> = vec![Vec::new(); HOMOLOGS];
        for h in 0..HOMOLOGS {
            rows[perm[h] as usize] = haps.row(h).to_vec();
        }
        if rows_ascend(&rows) {
            out.push(perm);
        }
    }
    out
}

/// The canonical relabelling of one parent: ascending rows, then the least
/// path signature, then the least image tuple.
fn canonical_perm(answer: &Reconstruction, parent: Parent) -> [u8; HOMOLOGS] {
    let mut best: Option<(Vec<u8>, [u8; HOMOLOGS])> = None;
    for perm in sorting_permutations(answer, parent) {
        let candidate = (signature(answer, parent, &perm), perm);
        if best.as_ref().is_none_or(|b| candidate < *b) {
            best = Some(candidate);
        }
    }
    best.expect("a sorted relabelling always exists").1
}

/// Rewrites a reconstruction into the form the result format demands.
pub fn canonicalise(answer: &Reconstruction) -> Reconstruction {
    let mut out = answer.clone();
    for parent in Parent::BOTH {
        let perm = canonical_perm(&out, parent);
        let rows: [Vec<u8>; HOMOLOGS] = std::array::from_fn(|h| out.haplotypes(parent).row(h).to_vec());
        let mut moved: [Vec<u8>; HOMOLOGS] = std::array::from_fn(|_| Vec::new());
        for h in 0..HOMOLOGS {
            moved[perm[h] as usize] = rows[h].clone();
        }
        *out.haplotypes_mut(parent) = Haplotypes::new(moved);
        for path in out.paths.iter_mut() {
            let states = path.states(parent).to_vec();
            *path.states_mut(parent) = states.iter().map(|s| s.remap(&perm)).collect();
        }
    }
    out
}

/// The key that orders reconstructions which score the same.
pub fn distinct_key(answer: &Reconstruction) -> Vec<u8> {
    let mut key = Vec::new();
    for parent in Parent::BOTH {
        for h in 0..HOMOLOGS {
            key.extend_from_slice(answer.haplotypes(parent).row(h));
        }
    }
    for path in &answer.paths {
        for parent in Parent::BOTH {
            for state in path.states(parent) {
                key.push(state.low());
                key.push(state.high());
            }
        }
    }
    key
}

/// Haplotypes of one parent from one chosen column per marker.
fn haplotypes_from(columns: &[[u8; HOMOLOGS]]) -> Haplotypes {
    let mut haps = Haplotypes::zeros(columns.len());
    for (m, column) in columns.iter().enumerate() {
        for h in 0..HOMOLOGS {
            haps.set(h, m, column[h]);
        }
    }
    haps
}

pub fn haplotypes_a(model: &Model, assign: &[(usize, usize)]) -> Haplotypes {
    let columns: Vec<[u8; HOMOLOGS]> =
        (0..model.markers).map(|m| model.cols_a[m][assign[m].0]).collect();
    haplotypes_from(&columns)
}

pub fn haplotypes_b(model: &Model, assign: &[(usize, usize)]) -> Haplotypes {
    let columns: Vec<[u8; HOMOLOGS]> =
        (0..model.markers).map(|m| model.cols_b[m][assign[m].1]).collect();
    haplotypes_from(&columns)
}

/// How close a partial total may sit to the optimum and still count as it.
const PATH_TOL: f64 = 1e-9;

/// Builds the reconstruction for an assignment, taking the least optimal path.
///
/// The assignment fixes the score. What is left is which of the paths that
/// attain it to report, and `spec/result-format-v1.md` wants the least
/// signature: progeny in index order, and within a progeny parent A's states
/// before parent B's. Progeny are independent once the parents are fixed, so
/// the least concatenation is the least block per progeny in turn.
///
/// `perm_a` and `perm_b` are the relabellings the states are compared under;
/// the caller tries every pair that sorts the rows, which is what makes the
/// comparison match the canonical form rather than one labelling of it.
pub fn build_paths(
    model: &Model,
    assign: &[(usize, usize)],
    perm_a: &[u8; HOMOLOGS],
    perm_b: &[u8; HOMOLOGS],
) -> Vec<ProgenyPath> {
    let m = model.markers;
    let order_a = state_order(perm_a);
    let order_b = state_order(perm_b);
    let mut nodes: Vec<[f64; NJ]> = Vec::with_capacity(m);
    let mut paths = Vec::with_capacity(model.offspring);
    for c in 0..model.offspring {
        model.nodes_for(c, assign, &mut nodes);
        paths.push(least_path(model, &nodes, &order_a, &order_b));
    }
    paths
}

/// Least optimal path through one progeny's node rows.
///
/// `order_a` and `order_b` list the ten states from least to greatest in the
/// terms the signature is written in.
fn least_path(
    model: &Model,
    nodes: &[[f64; NJ]],
    order_a: &[usize; NS],
    order_b: &[usize; NS],
) -> ProgenyPath {
    let m = model.markers;
    let neg = f64::NEG_INFINITY;

    // Best continuation from each joint state at each marker.
    let mut back: Vec<[f64; NJ]> = vec![[0.0f64; NJ]; m];
    for mm in (0..m - 1).rev() {
        let mut row = [neg; NJ];
        for sat in 0..NS {
            for sbt in 0..NS {
                let j = sat * NS + sbt;
                if !model.legal[mm + 1][j] {
                    continue;
                }
                let onward = nodes[mm + 1][j] + back[mm + 1][j];
                for sa in 0..NS {
                    for sb in 0..NS {
                        let jj = sa * NS + sb;
                        let v = model.ta[mm][sa][sat] + model.tb[mm][sb][sbt] + onward;
                        if v > row[jj] {
                            row[jj] = v;
                        }
                    }
                }
            }
        }
        back[mm] = row;
    }

    let mut best = neg;
    for j in 0..NJ {
        let v = nodes[0][j] + back[0][j];
        if v > best {
            best = v;
        }
    }

    // Parent A's states first: at each marker take the least state from which
    // the optimum is still reachable. The B side stays a vector over its own
    // states, because which of them pairs with A is settled later.
    let mut from_a = vec![0usize; m];
    let mut over_b = [neg; NS];
    for mm in 0..m {
        let mut chosen = 0usize;
        let mut chosen_b = [neg; NS];
        for &ua in order_a.iter() {
            let mut candidate = [neg; NS];
            if mm == 0 {
                for vb in 0..NS {
                    candidate[vb] = nodes[0][ua * NS + vb];
                }
            } else {
                let ta = &model.ta[mm - 1];
                let shift = ta[from_a[mm - 1]][ua];
                for vb in 0..NS {
                    let mut best_prev = neg;
                    let tb = &model.tb[mm - 1];
                    for pb in 0..NS {
                        let v = over_b[pb] + shift + tb[pb][vb];
                        if v > best_prev {
                            best_prev = v;
                        }
                    }
                    candidate[vb] = best_prev + nodes[mm][ua * NS + vb];
                }
            }
            let mut reachable = neg;
            for vb in 0..NS {
                let v = candidate[vb] + back[mm][ua * NS + vb];
                if v > reachable {
                    reachable = v;
                }
            }
            if reachable >= best - PATH_TOL {
                chosen = ua;
                chosen_b = candidate;
                break;
            }
        }
        from_a[mm] = chosen;
        over_b = chosen_b;
    }

    // Parent B's states, with A fixed: the same greedy walk one chain down.
    let mut node_b: Vec<[f64; NS]> = vec![[neg; NS]; m];
    for mm in 0..m {
        let mut row = [neg; NS];
        for vb in 0..NS {
            row[vb] = nodes[mm][from_a[mm] * NS + vb];
        }
        if mm > 0 {
            let shift = model.ta[mm - 1][from_a[mm - 1]][from_a[mm]];
            for slot in row.iter_mut() {
                *slot += shift;
            }
        }
        node_b[mm] = row;
    }
    let mut back_b: Vec<[f64; NS]> = vec![[0.0f64; NS]; m];
    for mm in (0..m - 1).rev() {
        let mut row = [neg; NS];
        let tb = &model.tb[mm];
        for pb in 0..NS {
            for vb in 0..NS {
                let v = tb[pb][vb] + node_b[mm + 1][vb] + back_b[mm + 1][vb];
                if v > row[pb] {
                    row[pb] = v;
                }
            }
        }
        back_b[mm] = row;
    }

    let mut from_b = vec![0usize; m];
    let mut prefix = 0.0f64;
    for mm in 0..m {
        let mut chosen = 0usize;
        for &vb in order_b.iter() {
            let here = if mm == 0 {
                node_b[0][vb]
            } else {
                prefix + model.tb[mm - 1][from_b[mm - 1]][vb] + node_b[mm][vb]
            };
            if here + back_b[mm][vb] >= best - PATH_TOL {
                chosen = vb;
                prefix = here;
                break;
            }
        }
        from_b[mm] = chosen;
    }

    ProgenyPath {
        from_a: from_a.iter().map(|&i| ALL_STATES[i]).collect(),
        from_b: from_b.iter().map(|&i| ALL_STATES[i]).collect(),
    }
}
