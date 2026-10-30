
//! The search.
//!
//! **This is the solver entry point you are asked to write.** You may also add
//! Rust helper modules beneath `src/solver/`. Everything else in the crate is
//! frozen: the scorer defines the objective, the reader validates the input,
//! and the writer encodes the answer. What is missing is the part that finds
//! the answer.
//!
//! `solve` must return the reconstruction that maximises the total log
//! likelihood over every parental haplotype assignment consistent with
//! `parent_dosage` and every legal inheritance state path, reduced to canonical
//! form as `spec/result-format-v1.md` describes.
//!
//! The placeholder below is a starting point, not a partial answer. It fixes
//! each parent's alleles by taking the lowest numbered homologs at every marker,
//! then runs each progeny's best path against that fixed guess, and returns
//! whatever homolog labelling that happened to produce.

use crate::cross_input::Cross;
use crate::cross_model::{
    Haplotypes, Parent, ProgenyPath, Reconstruction, State, ALL_STATES, HOMOLOGS,
};
use crate::scoring::{double_reduction_term, progeny_dosage, transition_term};

/// Finds the highest scoring reconstruction for a cross.
///
/// The returned reconstruction must be structurally valid, globally optimal,
/// and in canonical form.
pub fn solve(cross: &Cross) -> Reconstruction {
    let haplotypes_a = lowest_homologs_first(cross, Parent::A);
    let haplotypes_b = lowest_homologs_first(cross, Parent::B);

    let mut answer = Reconstruction {
        haplotypes_a,
        haplotypes_b,
        paths: Vec::with_capacity(cross.offspring),
    };
    for c in 0..cross.offspring {
        let path = best_path_for(cross, &answer, c);
        answer.paths.push(path);
    }
    answer
}

/// Places each marker's alternate alleles on the lowest numbered homologs.
///
/// This satisfies `parent_dosage` at every marker, which makes it admissible.
fn lowest_homologs_first(cross: &Cross, parent: Parent) -> Haplotypes {
    let mut haps = Haplotypes::zeros(cross.markers);
    let dosage = &cross.params(parent).dosage;
    for (m, &d) in dosage.iter().enumerate() {
        for h in 0..usize::from(d) {
            haps.set(h, m, 1);
        }
    }
    haps
}

/// Best inheritance path for one progeny against a fixed parental assignment.
///
/// A Viterbi pass over the hundred joint states against the haplotypes it is
/// given.
fn best_path_for(cross: &Cross, answer: &Reconstruction, progeny: usize) -> ProgenyPath {
    let joint: Vec<(State, State)> = ALL_STATES
        .iter()
        .flat_map(|a| ALL_STATES.iter().map(move |b| (*a, *b)))
        .collect();

    let legal = |sa: State, sb: State, m: usize| -> bool {
        (!sa.is_double_reduction() || cross.params(Parent::A).dr_allowed[m])
            && (!sb.is_double_reduction() || cross.params(Parent::B).dr_allowed[m])
    };

    let node_score = |sa: State, sb: State, m: usize| -> f64 {
        let dosage = progeny_dosage(answer, sa, sb, m);
        cross.child_gl[progeny][m][usize::from(dosage)]
            + double_reduction_term(cross, Parent::A, sa, m)
            + double_reduction_term(cross, Parent::B, sb, m)
    };

    let mut best = vec![f64::NEG_INFINITY; joint.len()];
    let mut back: Vec<Vec<usize>> = vec![vec![usize::MAX; joint.len()]; cross.markers];

    for (i, (sa, sb)) in joint.iter().enumerate() {
        if legal(*sa, *sb, 0) {
            best[i] = node_score(*sa, *sb, 0);
        }
    }

    // `m` is the marker index: it selects the legality mask, the emission and the
    // interval before it, and only incidentally the backpointer row.
    #[allow(clippy::needless_range_loop)]
    for m in 1..cross.markers {
        let mut next = vec![f64::NEG_INFINITY; joint.len()];
        for (i, (sa, sb)) in joint.iter().enumerate() {
            if !legal(*sa, *sb, m) {
                continue;
            }
            let here = node_score(*sa, *sb, m);
            for (j, (pa, pb)) in joint.iter().enumerate() {
                if best[j] == f64::NEG_INFINITY {
                    continue;
                }
                let step = best[j]
                    + transition_term(cross, Parent::A, *pa, *sa, m - 1)
                    + transition_term(cross, Parent::B, *pb, *sb, m - 1);
                if step + here > next[i] {
                    next[i] = step + here;
                    back[m][i] = j;
                }
            }
        }
        best = next;
    }

    let mut cursor = 0;
    for i in 1..joint.len() {
        if best[i] > best[cursor] {
            cursor = i;
        }
    }

    let mut from_a = vec![ALL_STATES[0]; cross.markers];
    let mut from_b = vec![ALL_STATES[0]; cross.markers];
    for m in (0..cross.markers).rev() {
        let (sa, sb) = joint[cursor];
        from_a[m] = sa;
        from_b[m] = sb;
        if m > 0 {
            cursor = back[m][cursor];
        }
    }
    ProgenyPath { from_a, from_b }
}

/// All permutations of the four homolog indices, in ascending image order.
///
/// Provided because canonical form is defined over this set and enumerating it
/// by hand is error prone. Which permutation to choose is the part that matters,
/// and that is not decided here.
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
