
//! Groups D, E, F and G: double reduction legality and homolog label symmetry.

use crate::canonical_check::{canonical_form, is_canonical};
use crate::harness::Outcome;
use crate::support;
use verifier_core::cross_model::{Parent, Reconstruction, HOMOLOGS};
use verifier_core::probe_plan::ALL_FIXTURES;
use verifier_core::scoring::score;

/// D: no progeny inherits the same homolog twice at a marker where that
/// parent's `double_reduction.allowed` is false.
///
/// A solver that treats double reduction as universally available fails here.
pub fn d_double_reduction_never_appears_where_forbidden() -> Outcome {
    for stem in ALL_FIXTURES {
        let fixture = support::load(stem);
        let produced = answer!(stem);

        for parent in Parent::BOTH {
            let allowed = &fixture.cross.params(parent).dr_allowed;
            for (c, path) in produced.paths.iter().enumerate() {
                for (m, state) in path.states(parent).iter().enumerate() {
                    demand!(
                        !state.is_double_reduction() || allowed[m],
                        "{stem}: progeny {c} inherits homolog {} twice from parent {} at \
                         marker {m}, where double reduction is not allowed",
                        state.low(),
                        parent.label()
                    );
                }
            }
        }
    }
    Outcome::Pass
}

/// E: double reduction is used at the markers where it is permitted and the
/// likelihood prefers it.
///
/// The optimum for the `dr_legality`, `compound` and `marker0_dr` fixtures
/// contains double reduction states, so a solver that never emits one cannot
/// reach it. This is the counterpart to group D: forbidding the form everywhere
/// is as wrong as allowing it everywhere. In `marker0_dr` the form is used at
/// the first marker, where no transition precedes the state.
pub fn e_double_reduction_is_used_where_it_is_optimal() -> Outcome {
    for stem in ["dr_legality", "compound", "marker0_dr"] {
        let fixture = support::load(stem);
        let produced = answer!(stem);

        let uses_dr = |answer: &Reconstruction| {
            answer.paths.iter().any(|path| {
                Parent::BOTH
                    .iter()
                    .any(|p| path.states(*p).iter().any(|s| s.is_double_reduction()))
            })
        };

        demand!(
            uses_dr(&fixture.expected),
            "{stem}: the verifier's own fixture no longer exercises double reduction"
        );
        demand!(
            uses_dr(&produced),
            "{stem}: the answer contains no double reduction state, but the optimum for \
             this cross does"
        );
        if let Some(problem) = support::compare(&fixture, &produced) {
            fail!("{stem}: {problem}");
        }
    }
    Outcome::Pass
}

/// F: the reported reconstruction is in the canonical form defined by
/// `spec/result-format-v1.md`.
///
/// Checked by enumerating all twenty four relabellings of each parent and
/// applying the specification's selection rules, independently of how the
/// candidate produced its answer.
pub fn f_answers_are_in_canonical_form() -> Outcome {
    for stem in ALL_FIXTURES {
        // Canonicity is a property of the reconstruction alone, so this group
        // needs the answer and not the cross it came from.
        let produced = answer!(stem);

        demand!(
            is_canonical(&produced),
            "{stem}: the answer is not the canonical representative of its relabelling class"
        );
    }
    Outcome::Pass
}

/// G: canonicalisation remaps inheritance paths consistently with the homolog
/// rows, handles repeated rows, and is idempotent.
///
/// Relabelling the produced answer and canonicalising it again must return the
/// produced answer, and must leave the score untouched. A solver that reorders
/// the haplotype rows without rewriting the states it already emitted fails.
pub fn g_canonicalisation_remaps_paths_and_is_idempotent() -> Outcome {
    // Two four-cycles, so no homolog keeps its label under either.
    let relabellings: [[u8; HOMOLOGS]; 2] = [[1, 2, 3, 0], [3, 0, 1, 2]];

    for stem in ALL_FIXTURES {
        let fixture = support::load(stem);
        let produced = answer!(stem);

        let Ok(baseline) = score(&fixture.cross, &produced) else {
            fail!("{stem}: the scorer rejects the answer");
        };

        demand!(
            canonical_form(&produced) == produced,
            "{stem}: canonicalising the answer again changes it, so it was not idempotent"
        );

        for perm in relabellings {
            let mut shuffled = produced.clone();
            for parent in Parent::BOTH {
                let rows = shuffled.haplotypes(parent).remap(&perm);
                *shuffled.haplotypes_mut(parent) = rows;
                for path in shuffled.paths.iter_mut() {
                    for state in path.states_mut(parent).iter_mut() {
                        *state = state.remap(&perm);
                    }
                }
            }

            let Ok(shuffled_score) = score(&fixture.cross, &shuffled) else {
                fail!("{stem}: relabelling the answer made it unscoreable, so the paths \
                       were not remapped with the rows");
            };
            demand!(
                (shuffled_score - baseline).abs() <= 1e-8,
                "{stem}: relabelling the homologs changed the score by {}",
                shuffled_score - baseline
            );
            demand!(
                canonical_form(&shuffled) == produced,
                "{stem}: a relabelling of the answer does not canonicalise back to it"
            );
        }
    }
    Outcome::Pass
}
