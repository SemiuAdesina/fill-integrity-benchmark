
//! An independent canonicaliser, owned by the verifier.
//!
//! This implements the six steps of the "Canonical form" section of
//! `spec/result-format-v1.md` directly, by enumerating all twenty four
//! relabellings of a parent and selecting among them. It is deliberately the
//! slow, literal reading of the specification: it shares no code with the
//! candidate's solver and none with the reference solution's symmetry
//! reduction, so agreement between them is evidence rather than tautology.
//!
//! Twenty four permutations per parent is cheap enough that there is no reason
//! to be clever here, and being clever is exactly how a canonicaliser acquires
//! the same blind spot as the thing it is checking.

use verifier_core::cross_model::{Parent, Reconstruction, HOMOLOGS};

/// All twenty four permutations of `{0,1,2,3}`, ascending as image tuples.
///
/// `perm[h]` is the index the row currently at `h` moves to, matching the
/// direction used by `Haplotypes::remap` and `State::remap`.
fn permutations() -> Vec<[u8; HOMOLOGS]> {
    let mut out = Vec::with_capacity(24);
    for a in 0..HOMOLOGS as u8 {
        for b in 0..HOMOLOGS as u8 {
            for c in 0..HOMOLOGS as u8 {
                for d in 0..HOMOLOGS as u8 {
                    let perm = [a, b, c, d];
                    let mut seen = [false; HOMOLOGS];
                    if perm.iter().all(|h| !std::mem::replace(&mut seen[*h as usize], true)) {
                        out.push(perm);
                    }
                }
            }
        }
    }
    out
}

/// The flat path signature for one parent under a relabelling.
///
/// Progeny in index order, markers in order, remapped `a` then remapped `b`.
fn signature(answer: &Reconstruction, parent: Parent, perm: &[u8; HOMOLOGS]) -> Vec<u8> {
    let mut out = Vec::with_capacity(answer.paths.len() * 2 * answer.haplotypes_a.markers());
    for path in &answer.paths {
        for state in path.states(parent) {
            let remapped = state.remap(perm);
            out.push(remapped.low());
            out.push(remapped.high());
        }
    }
    out
}

fn rows_ascending(rows: &[Vec<u8>; HOMOLOGS]) -> bool {
    rows.windows(2).all(|pair| pair[0] <= pair[1])
}

/// Canonicalises one parent in place, following the specification's steps.
fn canonicalise_parent(answer: &mut Reconstruction, parent: Parent) {
    // Step 3: only relabellings that leave the four rows ascending survive.
    let mut kept: Vec<[u8; HOMOLOGS]> = permutations()
        .into_iter()
        .filter(|perm| rows_ascending(answer.haplotypes(parent).remap(perm).rows()))
        .collect();

    // Step 4: among those, the ones minimising this parent's path signature.
    // More than one survives step 3 exactly when two rows are identical, and
    // then the rows cannot break the tie because swapping them changes nothing.
    if kept.len() > 1 {
        let best = kept
            .iter()
            .map(|perm| signature(answer, parent, perm))
            .min()
            .expect("at least one relabelling survives step 3");
        kept.retain(|perm| signature(answer, parent, perm) == best);
    }

    // Step 5: the lexicographically smallest image tuple settles what remains.
    kept.sort();
    let chosen = kept[0];

    // Step 6: reorder the rows and rewrite this parent's states.
    let remapped = answer.haplotypes(parent).remap(&chosen);
    *answer.haplotypes_mut(parent) = remapped;
    for path in answer.paths.iter_mut() {
        for state in path.states_mut(parent).iter_mut() {
            *state = state.remap(&chosen);
        }
    }
}

/// The canonical form of a reconstruction.
///
/// The two parents are canonicalised independently and are never exchanged.
pub fn canonical_form(answer: &Reconstruction) -> Reconstruction {
    let mut out = answer.clone();
    for parent in Parent::BOTH {
        canonicalise_parent(&mut out, parent);
    }
    out
}

/// True when a reconstruction is already in canonical form.
pub fn is_canonical(answer: &Reconstruction) -> bool {
    canonical_form(answer) == *answer
}

/// The ordering key from "Choosing between distinct optima".
///
/// Parent `A`'s canonical haplotype matrix flattened row major, then parent
/// `B`'s, then the full path signature: progeny in index order, and for each
/// progeny parent `A`'s states then parent `B`'s, each contributing `a` then
/// `b`. Comparing two keys lexicographically is comparing the reconstructions
/// in the documented order, first difference deciding.
pub fn ordering_key(answer: &Reconstruction) -> Vec<u8> {
    let markers = answer.haplotypes_a.markers();
    let mut key = Vec::with_capacity(2 * HOMOLOGS * markers + answer.paths.len() * 4 * markers);

    for parent in Parent::BOTH {
        let haps = answer.haplotypes(parent);
        for h in 0..HOMOLOGS {
            key.extend_from_slice(haps.row(h));
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
