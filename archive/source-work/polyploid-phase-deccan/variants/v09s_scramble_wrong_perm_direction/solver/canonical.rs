
//! Reducing a reconstruction to canonical form.
//!
//! Homolog indices are labels with no meaning of their own, so twenty-four
//! relabellings of a parent all describe the same biology and all score the
//! same. Exactly one of them is the accepted answer.
//!
//! Sorting the four rows is the obvious half and it is not sufficient. When two
//! homologs carry identical alleles at every marker, swapping them leaves the
//! sorted matrix untouched but rewrites every inheritance state that mentions
//! them. Row order alone therefore leaves the answer undetermined precisely when
//! rows repeat, and the paths have to settle it.

use crate::cross_model::{Parent, Reconstruction, HOMOLOGS};

/// Every permutation of the four homolog indices.
fn permutations() -> Vec<[u8; HOMOLOGS]> {
    let mut out = Vec::with_capacity(24);
    let mut current = [0u8; HOMOLOGS];
    let mut used = [false; HOMOLOGS];
    fill(0, &mut current, &mut used, &mut out);
    out
}

fn fill(
    depth: usize,
    current: &mut [u8; HOMOLOGS],
    used: &mut [bool; HOMOLOGS],
    out: &mut Vec<[u8; HOMOLOGS]>,
) {
    if depth == HOMOLOGS {
        out.push(*current);
        return;
    }
    for image in 0..HOMOLOGS as u8 {
        if used[image as usize] {
            continue;
        }
        used[image as usize] = true;
        current[depth] = image;
        fill(depth + 1, current, used, out);
        used[image as usize] = false;
    }
}

/// Reduces a reconstruction to canonical form in place.
///
/// Each parent is treated independently and the two are never exchanged.
pub fn canonicalise(answer: &mut Reconstruction) {
    for parent in Parent::BOTH {
        let perm = choose_permutation(answer, parent);
        apply(answer, parent, &perm);
    }
}

/// Picks the permutation that canonical form requires for one parent.
///
/// Applies the three rules in order: ascending rows, then smallest path
/// signature, then smallest image tuple.
fn choose_permutation(answer: &Reconstruction, parent: Parent) -> [u8; HOMOLOGS] {
    let haps = answer.haplotypes(parent);
    let markers = haps.markers();

    // Rule one: keep only permutations that sort the rows ascending.
    let mut sorted_rows: Vec<[u8; HOMOLOGS]> = Vec::new();
    for perm in permutations() {
        let mut rows: Vec<&[u8]> = vec![&[]; HOMOLOGS];
        for h in 0..HOMOLOGS {
            rows[perm[h] as usize] = haps.row(h);
        }
        if rows.windows(2).all(|w| w[0] <= w[1]) {
            sorted_rows.push(perm);
        }
    }

    if sorted_rows.len() == 1 {
        return sorted_rows[0];
    }

    // Rule two: among those, minimise the path signature. More than one
    // permutation survives rule one exactly when some rows are identical.
    let mut best: Option<([u8; HOMOLOGS], Vec<u8>)> = None;
    for perm in sorted_rows {
        let signature = path_signature(answer, parent, &perm, markers);
        let replace = match &best {
            None => true,
            Some((best_perm, best_sig)) => match signature.cmp(best_sig) {
                std::cmp::Ordering::Less => true,
                std::cmp::Ordering::Greater => false,
                // Rule three: identical signatures are settled by the image
                // tuple, so the choice never depends on enumeration order.
                std::cmp::Ordering::Equal => perm < *best_perm,
            },
        };
        if replace {
            best = Some((perm, signature));
        }
    }
    best.expect("at least one permutation sorts the rows").0
}

/// The flat state sequence for one parent under a candidate relabelling.
///
/// Progeny in index order, markers in order, remapped low index then remapped
/// high index.
fn path_signature(
    answer: &Reconstruction,
    parent: Parent,
    perm: &[u8; HOMOLOGS],
    markers: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(answer.paths.len() * markers * 2);
    for path in &answer.paths {
        for state in path.states(parent) {
            let remapped = state.remap(perm);
            out.push(remapped.low());
            out.push(remapped.high());
        }
    }
    out
}

/// Applies a relabelling to one parent's rows and to every state naming it.
fn apply(answer: &mut Reconstruction, parent: Parent, perm: &[u8; HOMOLOGS]) {
    let mut inv = [0u8; HOMOLOGS];
    for h in 0..HOMOLOGS { inv[perm[h] as usize] = h as u8; }
    let remapped = answer.haplotypes(parent).remap(&inv);
    *answer.haplotypes_mut(parent) = remapped;
    for path in &mut answer.paths {
        for state in path.states_mut(parent).iter_mut() {
            *state = state.remap(perm);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cross_model::{Haplotypes, ProgenyPath, State};

    fn reconstruction(rows_a: [&[u8]; HOMOLOGS], states_a: &[(u8, u8)]) -> Reconstruction {
        let markers = rows_a[0].len();
        let haps_a = Haplotypes::new([
            rows_a[0].to_vec(),
            rows_a[1].to_vec(),
            rows_a[2].to_vec(),
            rows_a[3].to_vec(),
        ]);
        Reconstruction {
            haplotypes_a: haps_a,
            haplotypes_b: Haplotypes::zeros(markers),
            paths: vec![ProgenyPath {
                from_a: states_a
                    .iter()
                    .map(|(a, b)| State::new(*a, *b).expect("in range"))
                    .collect(),
                from_b: vec![State::new(0, 1).expect("in range"); markers],
            }],
        }
    }

    #[test]
    fn sorts_rows_and_remaps_the_paths_that_name_them() {
        let mut answer = reconstruction(
            [&[1, 1], &[0, 0], &[0, 1], &[1, 0]],
            &[(0, 1), (0, 3)],
        );
        canonicalise(&mut answer);

        let rows: Vec<&[u8]> = (0..HOMOLOGS).map(|h| answer.haplotypes_a.row(h)).collect();
        assert!(rows.windows(2).all(|w| w[0] <= w[1]), "rows ascend");
        // Row 0 held `11` and moved to index 3; row 1 held `00` and moved to 0.
        assert_eq!(answer.paths[0].from_a[0], State::new(0, 3).expect("in range"));
    }

    #[test]
    fn repeated_rows_are_settled_by_the_path_signature() {
        // Rows 0 and 1 are identical, so sorting alone cannot choose between
        // the identity and the swap. The signature has to decide.
        let mut answer = reconstruction(
            [&[0, 0], &[0, 0], &[1, 0], &[1, 1]],
            &[(1, 2), (1, 2)],
        );
        canonicalise(&mut answer);
        assert_eq!(answer.paths[0].from_a[0], State::new(0, 2).expect("in range"));
    }

    #[test]
    fn canonicalising_twice_changes_nothing() {
        let mut once = reconstruction(
            [&[1, 0], &[0, 0], &[0, 1], &[1, 1]],
            &[(0, 2), (1, 3)],
        );
        canonicalise(&mut once);
        let mut twice = once.clone();
        canonicalise(&mut twice);
        assert_eq!(once, twice);
    }
}
