//! Canonical form and the order among distinct optima.
//!
//! `spec/result-format-v1.md` fixes one relabelling of every reconstruction:
//! minimise the remapped rows, then that parent's path signature, then the
//! image tuple. Minimising the triple over all twenty four permutations does
//! all three steps at once, because every permutation that leaves the rows
//! ascending produces the same sorted rows.

use crate::cross_model::{Haplotypes, Parent, Reconstruction, State, HOMOLOGS};

/// All permutations of the four homolog labels.
pub fn permutations() -> Vec<[u8; HOMOLOGS]> {
    let mut out = Vec::with_capacity(24);
    for a in 0..HOMOLOGS as u8 {
        for b in 0..HOMOLOGS as u8 {
            for c in 0..HOMOLOGS as u8 {
                for d in 0..HOMOLOGS as u8 {
                    let p = [a, b, c, d];
                    let mut seen = [false; HOMOLOGS];
                    if p.iter().all(|x| {
                        let fresh = !seen[*x as usize];
                        seen[*x as usize] = true;
                        fresh
                    }) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

/// The value a reconstruction's relabelling is ordered by, for one parent.
pub fn relabel_key(
    answer: &Reconstruction,
    parent: Parent,
    p: &[u8; HOMOLOGS],
) -> (Vec<Vec<u8>>, Vec<u8>, [u8; HOMOLOGS]) {
    let haps = answer.haplotypes(parent);
    let mut rows = vec![Vec::new(); HOMOLOGS];
    for h in 0..HOMOLOGS {
        rows[p[h] as usize] = haps.row(h).to_vec();
    }
    let mut signature = Vec::new();
    for path in &answer.paths {
        for s in path.states(parent) {
            let r = s.remap(p);
            signature.push(r.low());
            signature.push(r.high());
        }
    }
    (rows, signature, *p)
}

/// The canonical relabelling of a reconstruction.
pub fn canonicalise(answer: &Reconstruction) -> Reconstruction {
    let mut out = answer.clone();
    for parent in Parent::BOTH {
        let mut best: Option<((Vec<Vec<u8>>, Vec<u8>, [u8; HOMOLOGS]), [u8; HOMOLOGS])> = None;
        for p in permutations() {
            let key = relabel_key(&out, parent, &p);
            if best.as_ref().map_or(true, |(b, _)| &key < b) {
                best = Some((key, p));
            }
        }
        let p = best.expect("there is always a permutation").1;
        let haps = out.haplotypes(parent).remap(&p);
        *out.haplotypes_mut(parent) = haps;
        for path in out.paths.iter_mut() {
            for s in path.states_mut(parent).iter_mut() {
                *s = s.remap(&p);
            }
        }
    }
    out
}

/// Why a reconstruction is not canonical, if it is not.
pub fn canonical_defect(answer: &Reconstruction) -> Option<String> {
    let identity = [0u8, 1, 2, 3];
    for parent in Parent::BOTH {
        let own = relabel_key(answer, parent, &identity);
        if !own.0.windows(2).all(|w| w[0] <= w[1]) {
            return Some(format!("parent {} rows are not ascending", parent.label()));
        }
        for p in permutations() {
            if relabel_key(answer, parent, &p) < own {
                return Some(format!(
                    "parent {} has an earlier relabelling than the answer",
                    parent.label()
                ));
            }
        }
    }
    None
}

/// The order among distinct optima: parent A's rows, parent B's rows, then the
/// full path signature with each progeny's parent A states before parent B's.
pub fn distinct_key(answer: &Reconstruction) -> Vec<u8> {
    let mut key = Vec::new();
    for parent in Parent::BOTH {
        for h in 0..HOMOLOGS {
            key.extend_from_slice(answer.haplotypes(parent).row(h));
        }
    }
    for path in &answer.paths {
        for parent in Parent::BOTH {
            for s in path.states(parent) {
                key.push(s.low());
                key.push(s.high());
            }
        }
    }
    key
}

/// Row sorting permutations: those that leave a sorted haplotype matrix sorted.
///
/// Rows that carry identical alleles at every marker may be exchanged without
/// touching the matrix, so these are exactly the permutations the canonical
/// form has to choose between.
pub fn row_permutations(haps: &Haplotypes) -> Vec<[u8; HOMOLOGS]> {
    let mut out = Vec::new();
    for p in permutations() {
        let remapped = haps.remap(&p);
        let ascending = (0..HOMOLOGS - 1)
            .all(|h| remapped.row(h) <= remapped.row(h + 1));
        if ascending {
            out.push(p);
        }
    }
    out
}

/// Where each canonical state index lands under a relabelling, as the sorted
/// pair of new homolog indices.
pub fn remap_table(p: &[u8; HOMOLOGS]) -> Vec<(u8, u8)> {
    use crate::cross_model::ALL_STATES;
    ALL_STATES
        .iter()
        .map(|s| {
            let a = p[s.low() as usize];
            let b = p[s.high() as usize];
            if a <= b {
                (a, b)
            } else {
                (b, a)
            }
        })
        .collect()
}

/// Rebuilds a reconstruction's rows from a haplotype pair, keeping the paths.
pub fn with_haplotypes(
    answer: &Reconstruction,
    parent: Parent,
    haps: Haplotypes,
) -> Reconstruction {
    let mut out = answer.clone();
    *out.haplotypes_mut(parent) = haps;
    out
}

/// Unused import guard.
pub fn state_pair(a: u8, b: u8) -> Option<State> {
    State::new(a, b)
}
