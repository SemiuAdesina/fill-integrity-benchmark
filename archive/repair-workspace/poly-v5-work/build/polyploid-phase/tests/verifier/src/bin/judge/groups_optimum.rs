
//! Groups H, I, J and K: global optimality and deterministic tie behaviour.

use crate::canonical_check::{is_canonical, ordering_key};
use crate::greedy_baseline::marker_at_a_time_score;
use crate::harness::Outcome;
use crate::support;
use verifier_core::cross_model::{Parent, Reconstruction, ALL_STATES};
use verifier_core::probe_plan::ALL_FIXTURES;
use verifier_core::scoring::{score, scores_equal, SCORE_TOLERANCE};

/// H: every fixture is solved to the global maximum, in canonical form, with
/// the digest the specification requires.
pub fn h_every_fixture_reaches_the_global_optimum() -> Outcome {
    for stem in ALL_FIXTURES {
        let fixture = support::load(stem);
        let produced = answer!(stem);
        if let Some(problem) = support::compare(&fixture, &produced) {
            fail!("{stem}: {problem}");
        }
    }
    Outcome::Pass
}

/// I: the answer beats what a marker-at-a-time reconstruction reaches.
///
/// The verifier runs such a solver rather than assuming it fails: each marker's
/// parental allele columns are chosen on their own, and each progeny's path is
/// then recovered exactly. Its score is asserted to fall short of the optimum
/// before the candidate is graded, so the fixture cannot quietly stop
/// separating the two.
pub fn i_marker_at_a_time_reconstruction_falls_short() -> Outcome {
    for stem in ["greedy_trap", "compound"] {
        let fixture = support::load(stem);

        let greedy = marker_at_a_time_score(&fixture.cross);
        demand!(
            greedy + SCORE_TOLERANCE < fixture.optimum,
            "{stem}: the verifier's own fixture no longer separates a marker-at-a-time \
             reconstruction, which scores {greedy:.10}, from the optimum {:.10}",
            fixture.optimum
        );

        let produced = answer!(stem);
        if let Some(problem) = support::compare(&fixture, &produced) {
            fail!("{stem}: {problem}");
        }
    }
    Outcome::Pass
}

/// J: the recombination terms across intervals are part of the objective.
///
/// The optimum for these fixtures contains intervals where a progeny's
/// inheritance state changes, so an answer that holds each progeny's state
/// constant across the chromosome, or that scores markers in isolation, cannot
/// match it.
pub fn j_recombination_between_markers_is_accounted_for() -> Outcome {
    for stem in ["recombination", "compound"] {
        let fixture = support::load(stem);
        let produced = answer!(stem);

        let switches = |answer: &Reconstruction| {
            answer
                .paths
                .iter()
                .flat_map(|path| Parent::BOTH.iter().map(move |p| path.states(*p)))
                .flat_map(|states| states.windows(2))
                .filter(|pair| pair[0].switch_count(pair[1]) > 0)
                .count()
        };

        demand!(
            switches(&fixture.expected) > 0,
            "{stem}: the verifier's own fixture no longer contains a recombination event"
        );
        demand!(
            switches(&produced) > 0,
            "{stem}: no progeny changes inheritance state at any interval, but the optimum \
             for this cross does"
        );
        if let Some(problem) = support::compare(&fixture, &produced) {
            fail!("{stem}: {problem}");
        }
    }
    Outcome::Pass
}

/// K: when reconstructions tie, the one required by the documented order is
/// returned, and repeated runs agree.
///
/// The agreement half is graded across two probe processes that were given the
/// same cross independently, which is why `probe_plan` carries a second job for
/// each of these three fixtures. It is the only reason any cross is solved more
/// than once in a graded run: the question is itself about repeated execution
/// and cannot be answered from a single answer.
///
/// The ordering half scores every single-state variation of the answer. If one
/// of them ties within `1e-8`, is structurally admissible, is canonical, and
/// sorts before the answer under "Choosing between distinct optima", then the
/// answer was the wrong representative of a tie rather than one of several
/// acceptable optima.
pub fn k_ties_resolve_to_the_documented_representative() -> Outcome {
    for stem in ["ties", "symmetry", "compound"] {
        let fixture = support::load(stem);
        let produced = answer!(stem);

        let replicate = answer!(&format!("{stem}_again"));
        demand!(
            replicate == produced,
            "{stem}: two runs on the same cross returned different answers"
        );

        // The tie scan below only asks whether some equally scoring variant sorts
        // earlier, which a wrong answer can satisfy without being right: vary a
        // state of a low scoring reconstruction and the variant usually scores
        // differently, so nothing ties and nothing is caught. Requiring the
        // optimum first is what makes the rest of this check about ties.
        if let Some(problem) = support::compare(&fixture, &produced) {
            fail!("{stem}: {problem}");
        }

        let Ok(baseline) = score(&fixture.cross, &produced) else {
            fail!("{stem}: the scorer rejects the answer");
        };
        let key = ordering_key(&produced);

        for c in 0..fixture.cross.offspring {
            for m in 0..fixture.cross.markers {
                for parent in Parent::BOTH {
                    for replacement in ALL_STATES {
                        let mut variant = produced.clone();
                        variant.paths[c].states_mut(parent)[m] = replacement;

                        let Ok(other) = score(&fixture.cross, &variant) else {
                            continue;
                        };
                        if !scores_equal(other, baseline) {
                            continue;
                        }
                        demand!(
                            !(is_canonical(&variant) && ordering_key(&variant) < key),
                            "{stem}: an equally scoring reconstruction sorts before the \
                             answer, so the tie was not broken as documented"
                        );
                    }
                }
            }
        }
    }
    Outcome::Pass
}
