
//! Canonical form and the order among distinct optima.
//!
//! `spec/result-format-v1.md` fixes both. Canonical form is chosen parent by
//! parent: among the twenty four relabellings, keep those that put the four
//! homolog rows in ascending order, then those that minimise the parent's own
//! path signature, then the one with the smallest image tuple. Distinct optima
//! are then ordered by parent A's rows, parent B's rows, then the full path
//! signature with each progeny's parent A states before its parent B states.

use crate::cross_model::{Haplotypes, Parent, Reconstruction, State, HOMOLOGS};

/// All twenty four permutations of the four homolog indices, ascending by image.
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

/// The key a single relabelling is ranked by: the reordered rows, the parent's
/// remapped path signature, then the image tuple.
fn relabel_key(hap: &Haplotypes, seq: &[Vec<State>], perm: &[u8; HOMOLOGS]) -> (Vec<u8>, Vec<u8>, [u8; HOMOLOGS]) {
    let markers = hap.markers();
    let mut rows = vec![0u8; HOMOLOGS * markers];
    for h in 0..HOMOLOGS {
        let dst = usize::from(perm[h]) * markers;
        for mm in 0..markers {
            rows[dst + mm] = hap.row(h)[mm];
        }
    }
    let mut signature = Vec::with_capacity(seq.len() * seq.first().map_or(0, Vec::len) * 2);
    for states in seq {
        for state in states {
            let moved = state.remap(perm);
            signature.push(moved.low());
            signature.push(moved.high());
        }
    }
    (rows, signature, *perm)
}

/// The relabelling the specification requires for one parent.
pub fn best_permutation(hap: &Haplotypes, seq: &[Vec<State>]) -> [u8; HOMOLOGS] {
    let mut best_perm = [0u8, 1, 2, 3];
    let mut best_key: Option<(Vec<u8>, Vec<u8>, [u8; HOMOLOGS])> = None;
    for perm in permutations() {
        let key = relabel_key(hap, seq, &perm);
        if best_key.as_ref().is_none_or(|held| key < *held) {
            best_key = Some(key);
            best_perm = perm;
        }
    }
    best_perm
}

/// Rewrites the reconstruction into canonical form, in place.
pub fn canonicalise(answer: &mut Reconstruction) {
    for parent in Parent::BOTH {
        let seq: Vec<Vec<State>> = answer.paths.iter().map(|p| p.states(parent).to_vec()).collect();
        let perm = best_permutation(answer.haplotypes(parent), &seq);
        if perm == [0u8, 1, 2, 3] {
            continue;
        }
        let moved = answer.haplotypes(parent).remap(&perm);
        *answer.haplotypes_mut(parent) = moved;
        for path in answer.paths.iter_mut() {
            let states = path.states_mut(parent);
            for state in states.iter_mut() {
                *state = state.remap(&perm);
            }
        }
    }
}

/// The permutations that leave one parent's rows in ascending order.
///
/// The canonical relabelling is always one of these: step 3 of the
/// specification keeps exactly the permutations whose rows come out sorted, and
/// steps 4 and 5 only choose among those.
pub fn sorting_permutations(hap: &Haplotypes) -> Vec<[u8; HOMOLOGS]> {
    let mut out = Vec::new();
    for perm in permutations() {
        let mut rows = vec![Vec::new(); HOMOLOGS];
        for h in 0..HOMOLOGS {
            rows[usize::from(perm[h])] = hap.row(h).to_vec();
        }
        if rows.windows(2).all(|w| w[0] <= w[1]) {
            out.push(perm);
        }
    }
    out
}

/// The key `spec/result-format-v1.md` minimises when two genuinely different
/// reconstructions score the same.
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
