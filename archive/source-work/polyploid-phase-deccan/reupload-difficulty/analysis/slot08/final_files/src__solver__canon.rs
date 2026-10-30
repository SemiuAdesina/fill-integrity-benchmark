// Canonical form, and the order among reconstructions that tie.
//
// Homolog labels are arbitrary, so a reconstruction is only ever accepted in
// one relabelling: the rows of each parent in ascending lexicographic order,
// and where equal rows leave a choice, the one that makes that parent's state
// path smallest. Two genuinely different reconstructions can still score
// identically, and `spec/result-format-v1.md` orders those by parent A's rows,
// then parent B's, then the whole path signature.

use crate::cross_model::{
    Haplotypes, Parent, ProgenyPath, Reconstruction, State, HOMOLOGS,
};

/// All twenty four permutations of the four homolog labels, ascending by image.
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
                        seen[usize::from(*x)] = true;
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

/// The rows of `parent` after `perm` moves row `h` to `perm[h]`.
fn remapped_rows(answer: &Reconstruction, parent: Parent, perm: &[u8; HOMOLOGS]) -> Vec<Vec<u8>> {
    let haps = answer.haplotypes(parent);
    let mut rows = vec![Vec::new(); HOMOLOGS];
    for h in 0..HOMOLOGS {
        rows[usize::from(perm[h])] = haps.row(h).to_vec();
    }
    rows
}

fn ascending(rows: &[Vec<u8>]) -> bool {
    rows.windows(2).all(|w| w[0] <= w[1])
}

/// The flat sequence of remapped states, progeny in index order then markers,
/// emitting the low homolog then the high one.
pub fn parent_signature(
    answer: &Reconstruction,
    parent: Parent,
    perm: &[u8; HOMOLOGS],
) -> Vec<u8> {
    let mut out = Vec::new();
    for path in &answer.paths {
        for state in path.states(parent) {
            let moved = state.remap(perm);
            out.push(moved.low());
            out.push(moved.high());
        }
    }
    out
}

/// The relabelling `spec/result-format-v1.md` requires for one parent: among
/// the permutations that leave the rows ascending, the one with the smallest
/// path signature, and then the smallest image tuple.
pub fn canonical_permutation(answer: &Reconstruction, parent: Parent) -> [u8; HOMOLOGS] {
    let mut best: Option<(Vec<u8>, [u8; HOMOLOGS])> = None;
    for perm in permutations() {
        if !ascending(&remapped_rows(answer, parent, &perm)) {
            continue;
        }
        let key = (parent_signature(answer, parent, &perm), perm);
        if best.as_ref().is_none_or(|current| key < *current) {
            best = Some(key);
        }
    }
    best.expect("the sorting permutation always leaves the rows ascending").1
}

/// The same reconstruction relabelled into canonical form.
pub fn canonicalize(answer: &Reconstruction) -> Reconstruction {
    let mut out = answer.clone();
    for parent in Parent::BOTH {
        let perm = canonical_permutation(answer, parent);
        let rows = remapped_rows(answer, parent, &perm);
        let mut moved = Haplotypes::zeros(answer.haplotypes(parent).markers());
        for (h, row) in rows.iter().enumerate() {
            for (m, allele) in row.iter().enumerate() {
                moved.set(h, m, *allele);
            }
        }
        *out.haplotypes_mut(parent) = moved;
        for path in out.paths.iter_mut() {
            let states: Vec<State> = path.states(parent).iter().map(|s| s.remap(&perm)).collect();
            *path.states_mut(parent) = states;
        }
    }
    out
}

/// The key `spec/result-format-v1.md` minimises between distinct optima:
/// parent A's rows, parent B's rows, then every progeny's parent A states
/// followed by its parent B states.
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

#[allow(dead_code)]
fn unused(path: &ProgenyPath) -> usize {
    path.from_a.len()
}
