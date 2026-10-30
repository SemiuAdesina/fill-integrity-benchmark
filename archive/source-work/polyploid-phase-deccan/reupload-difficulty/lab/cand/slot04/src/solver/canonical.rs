
//! Canonical form and the order among distinct optima.
//!
//! Homolog labels are arbitrary, so `spec/result-format-v1.md` fixes one
//! relabelling of each parent: the rows are sorted, ties are broken by that
//! parent's path signature, and then by the permutation's own image.

use crate::cross_model::{Haplotypes, Parent, Reconstruction, State, HOMOLOGS};

/// All permutations of the four homolog indices, in ascending image order.
pub fn homolog_permutations() -> Vec<[u8; HOMOLOGS]> {
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

/// Rows after moving the row at index `h` to index `perm[h]`.
fn remapped_rows(haps: &Haplotypes, perm: &[u8; HOMOLOGS]) -> Vec<Vec<u8>> {
    let mut rows = vec![Vec::new(); HOMOLOGS];
    for h in 0..HOMOLOGS {
        rows[perm[h] as usize] = haps.row(h).to_vec();
    }
    rows
}

/// Flat remapped state sequence: progeny ascending, markers ascending, the
/// remapped `a` then the remapped `b`.
fn path_signature(answer: &Reconstruction, parent: Parent, perm: &[u8; HOMOLOGS]) -> Vec<u8> {
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

/// Rewrites a reconstruction into canonical form, in place.
pub fn canonicalize(answer: &mut Reconstruction) {
    for parent in Parent::BOTH {
        let mut best: Option<(Vec<Vec<u8>>, Vec<u8>, [u8; HOMOLOGS])> = None;
        for perm in homolog_permutations() {
            let rows = remapped_rows(answer.haplotypes(parent), &perm);
            if !rows.windows(2).all(|w| w[0] <= w[1]) {
                continue;
            }
            let signature = path_signature(answer, parent, &perm);
            let key = (rows, signature, perm);
            if best.as_ref().map_or(true, |other| &key < other) {
                best = Some(key);
            }
        }
        let (rows, _, perm) = best.expect("at least one permutation sorts the rows");
        let markers = rows[0].len();
        let mut remapped = Haplotypes::zeros(markers);
        for h in 0..HOMOLOGS {
            for m in 0..markers {
                remapped.set(h, m, rows[h][m]);
            }
        }
        *answer.haplotypes_mut(parent) = remapped;
        for path in answer.paths.iter_mut() {
            for state in path.states_mut(parent).iter_mut() {
                *state = state.remap(&perm);
            }
        }
    }
}

/// The order `spec/result-format-v1.md` uses to pick between distinct optima:
/// parent `A`'s canonical rows, then parent `B`'s, then the full path
/// signature with each progeny's parent `A` states before parent `B`'s.
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

/// Two reconstructions in canonical form, ordered by [`distinct_key`].
pub fn key_less(left: &Reconstruction, right: &Reconstruction) -> bool {
    distinct_key(left) < distinct_key(right)
}

/// A state written the way the result format requires, for tests.
pub fn state_of(a: u8, b: u8) -> State {
    State::new(a, b).expect("homolog indices are within range")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::canonical_digest;
    use crate::result_output::decode;

    /// The worked example in `examples/` is the one published answer with
    /// duplicate homolog rows, so it decides whether steps 4 and 5 of canonical
    /// form - the ones the paths settle - agree with the specification.
    #[test]
    fn canonical_form_leaves_the_worked_example_alone() {
        let text = std::fs::read_to_string("/app/examples/small.result.json")
            .expect("the worked example is present");
        let decoded = decode(&text, 5, 3).expect("the worked example decodes");
        let example = decoded.answer;

        let mut again = example.clone();
        canonicalize(&mut again);
        assert_eq!(again, example, "canonical form moved the worked example");
        assert_eq!(
            canonical_digest(&example),
            decoded.canonical_digest,
            "the example does not digest to its own value"
        );

        // Relabelling is a symmetry of the score, so canonical form has to
        // undo it: every relabelling of the example canonicalises back to it.
        for perm_a in homolog_permutations() {
            for perm_b in homolog_permutations() {
                let mut moved = example.clone();
                *moved.haplotypes_mut(Parent::A) =
                    example.haplotypes(Parent::A).remap(&perm_a);
                *moved.haplotypes_mut(Parent::B) =
                    example.haplotypes(Parent::B).remap(&perm_b);
                for path in moved.paths.iter_mut() {
                    for state in path.states_mut(Parent::A).iter_mut() {
                        *state = state.remap(&perm_a);
                    }
                    for state in path.states_mut(Parent::B).iter_mut() {
                        *state = state.remap(&perm_b);
                    }
                }
                canonicalize(&mut moved);
                assert_eq!(moved, example, "relabelling {perm_a:?} {perm_b:?} did not canonicalise back");
            }
        }
    }
}
