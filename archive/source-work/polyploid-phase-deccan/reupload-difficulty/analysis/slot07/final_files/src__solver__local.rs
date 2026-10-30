
//! Local search over haplotype assignments.
//!
//! The branch and bound in `search` needs a good incumbent before it can prune
//! anything, so this module exists to supply one. `refine` alternates the two
//! moves that are cheap because each halves the problem: given an assignment,
//! every progeny's best path is a Viterbi pass, and given every progeny's path,
//! the best assignment is a per marker choice. `polish` then tries every single
//! marker substitution and re-optimises the paths each time, which escapes the
//! fixed points the alternation settles into.

use crate::solver::model::{Model, NEG, NJ, NS};

/// Sweeps of single marker substitution run from each start.
const SWEEPS: usize = 8;

/// Tiny deterministic generator, so a run reproduces exactly.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() >> 32) as usize % n
    }
}

/// Alternates best paths and best assignment until the assignment stops moving.
pub fn refine(model: &Model, z: &mut [usize]) -> f64 {
    let markers = model.markers;
    let offspring = model.offspring;
    let mut paths: Vec<Vec<usize>> = vec![Vec::with_capacity(markers); offspring];
    loop {
        let mut total = 0.0f64;
        for c in 0..offspring {
            let (score, path) = model.viterbi_path(c, z);
            total += score;
            paths[c] = path;
        }
        let mut moved = false;
        for m in 0..markers {
            let mut best_z = z[m];
            let mut best = f64::NEG_INFINITY;
            for cand in 0..model.zcount[m] {
                let mut sum = 0.0f64;
                let gl_m = &model.gl;
                let row = &model.dose[m][cand];
                for c in 0..offspring {
                    let j = paths[c][m];
                    if model.base[m][j].is_infinite() && model.base[m][j] < 0.0 {
                        continue;
                    }
                    sum += gl_m[c][m][usize::from(row[j])];
                }
                if sum > best {
                    best = sum;
                    best_z = cand;
                }
            }
            if best_z != z[m] {
                z[m] = best_z;
                moved = true;
            }
        }
        if !moved {
            return total;
        }
    }
}

/// Repeated single marker substitution, each judged against the whole cross.
///
/// For every marker and every column this asks what the cross would score if
/// that marker alone took that column, and moves to the best. The paths are
//! re-optimised rather than held fixed, which is what `refine` cannot do, and
/// it is affordable because one forward and one backward pass per progeny price
/// every candidate at every marker at once: with state `j` at marker `m` the
/// score of column `v` there is `A[j] + gl[dose_v[j]] + base[j] + B[j]`, where
/// `A` gathers the markers before and `B` the markers after.
pub fn polish(model: &Model, z: &mut [usize], sweeps: usize) -> f64 {
    let markers = model.markers;
    let offspring = model.offspring;
    let zmax = model.zcount.iter().copied().max().unwrap_or(1);
    let mut fwd = vec![[NEG; NJ]; markers];
    let mut bwd = vec![[NEG; NJ]; markers];
    let mut mid = [NEG; NJ];
    let mut node = [0.0f64; NJ];
    let mut rest = [NEG; NJ];
    let mut ahead = [NEG; NJ];
    let zero = [0.0f64; NJ];
    let mut gain = vec![vec![0.0f64; zmax]; markers];
    let mut proposed = vec![0usize; markers];

    // Every marker moves to its best column at once, which is not monotone on
    // its own: two markers can swap back and forth for ever. So a sweep is
    // judged as a whole by a fresh evaluation, the best assignment seen is
    // kept, and a sweep that fails to improve ends the run.
    let mut current = model.value(z);
    let mut best = current;
    let mut best_z = z.to_vec();
    for _ in 0..sweeps {
        for m in 0..markers {
            for slot in gain[m].iter_mut() {
                *slot = 0.0;
            }
        }
        for c in 0..offspring {
            for m in 0..markers {
                model.node_costs(c, m, z[m], &mut node);
                if m == 0 {
                    fwd[0].copy_from_slice(&node);
                } else {
                    let (prev, here) = (&fwd[m - 1], &mut fwd[m]);
                    model.advance(
                        &model.ta[m - 1],
                        &model.tb[m - 1],
                        prev,
                        &mut mid,
                        here,
                        &node,
                    );
                }
            }
            for j in 0..NJ {
                bwd[markers - 1][j] = 0.0;
            }
            for m in (0..markers - 1).rev() {
                model.node_costs(c, m + 1, z[m + 1], &mut node);
                for &j in model.legal[m + 1].iter() {
                    rest[j] = node[j] + bwd[m + 1][j];
                }
                backward(
                    &model.ta[m],
                    &model.tb[m],
                    &rest,
                    &mut mid,
                    &mut bwd[m],
                    &model.legal_a[m + 1],
                    &model.legal_b[m + 1],
                    &model.legal_a[m],
                    &model.legal_b[m],
                );
            }
            for m in 0..markers {
                if m == 0 {
                    for j in 0..NJ {
                        rest[j] = 0.0;
                    }
                } else {
                    model.advance(
                        &model.ta[m - 1],
                        &model.tb[m - 1],
                        &fwd[m - 1],
                        &mut mid,
                        &mut ahead,
                        &zero,
                    );
                    rest.copy_from_slice(&ahead);
                }
                let gl = &model.gl[c][m];
                let base = &model.base[m];
                let back = &bwd[m];
                for &j in model.legal[m].iter() {
                    rest[j] += base[j] + back[j];
                }
                for cand in 0..model.zcount[m] {
                    let dose = &model.dose[m][cand];
                    let mut best = NEG;
                    for &j in model.legal[m].iter() {
                        let v = rest[j] + gl[usize::from(dose[j])];
                        if v > best {
                            best = v;
                        }
                    }
                    gain[m][cand] += best;
                }
            }
        }
        let mut changed = false;
        let mut single_at = 0usize;
        let mut single_z = 0usize;
        let mut single_value = f64::NEG_INFINITY;
        for m in 0..markers {
            let mut take = z[m];
            let mut take_value = gain[m][z[m]];
            for cand in 0..model.zcount[m] {
                if gain[m][cand] > take_value + 1e-12 {
                    take_value = gain[m][cand];
                    take = cand;
                }
            }
            if take != z[m] {
                changed = true;
                if take_value > single_value {
                    single_value = take_value;
                    single_at = m;
                    single_z = take;
                }
            }
            proposed[m] = take;
        }
        if !changed {
            break;
        }
        let value = model.value(&proposed);
        if value > best + 1e-12 {
            best = value;
            best_z.copy_from_slice(&proposed);
        }
        if value > current + 1e-12 {
            current = value;
            z.copy_from_slice(&proposed);
            continue;
        }
        // Moving every marker at once overshoots. Take the one move that gains
        // the most on its own instead, and stop when even that is no gain.
        proposed.copy_from_slice(z);
        proposed[single_at] = single_z;
        let value = model.value(&proposed);
        if value > best + 1e-12 {
            best = value;
            best_z.copy_from_slice(&proposed);
        }
        if !(value > current + 1e-12) {
            break;
        }
        current = value;
        z[single_at] = single_z;
    }
    z.copy_from_slice(&best_z);
    best
}

/// `out[j] = max over j' of (T[j][j'] + rest[j'])`, `T` separable per parent.
fn backward(
    ta: &[f64; 100],
    tb: &[f64; 100],
    rest: &[f64; NJ],
    mid: &mut [f64; NJ],
    out: &mut [f64; NJ],
    src_a: &[u8],
    src_b: &[u8],
    dst_a: &[u8],
    dst_b: &[u8],
) {
    for &ap in src_a.iter() {
        let ap = usize::from(ap);
        for &b in dst_b.iter() {
            let b = usize::from(b);
            let mut best = NEG;
            for &bp in src_b.iter() {
                let v = rest[ap * 10 + usize::from(bp)] + tb[b * 10 + usize::from(bp)];
                if v > best {
                    best = v;
                }
            }
            mid[ap * 10 + b] = best;
        }
    }
    for &a in dst_a.iter() {
        let a = usize::from(a);
        for &b in dst_b.iter() {
            let b = usize::from(b);
            let mut best = NEG;
            for &ap in src_a.iter() {
                let v = mid[usize::from(ap) * 10 + b] + ta[a * 10 + usize::from(ap)];
                if v > best {
                    best = v;
                }
            }
            out[a * 10 + b] = best;
        }
    }
}

/// Best assignment found from `restarts` random starts, written into `z`.
pub fn search_local(model: &Model, restarts: usize, seed: u64) -> (Vec<usize>, f64) {
    let markers = model.markers;
    let mut rng = Rng::new(seed);
    let mut best_z: Vec<usize> = vec![0; markers];
    let mut best = f64::NEG_INFINITY;

    // One deterministic start: the lowest numbered homologs at every marker.
    let mut z = vec![0usize; markers];
    let value = refine(model, &mut z);
    polish(model, &mut z, SWEEPS);
    let value = model.value(&z);
    if value > best {
        best = value;
        best_z.copy_from_slice(&z);
    }

    for _ in 0..restarts {
        let mut z: Vec<usize> = (0..markers).map(|m| rng.below(model.zcount[m])).collect();
        refine(model, &mut z);
        polish(model, &mut z, SWEEPS);
        let value = model.value(&z);
        if value > best {
            best = value;
            best_z.copy_from_slice(&z);
        }
    }

    (best_z, best)
}

/// Scratch for the search; kept here so `model` stays immutable.
pub type Scratch = Vec<[f64; NJ]>;
