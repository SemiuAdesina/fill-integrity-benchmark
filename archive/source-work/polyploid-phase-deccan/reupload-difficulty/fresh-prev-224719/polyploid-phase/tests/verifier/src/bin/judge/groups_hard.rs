
//! Groups L and M: the compound case, and evidence the answer was derived.

use crate::greedy_baseline::marker_at_a_time_score;
use crate::harness::Outcome;
use crate::support;
use verifier_core::cross_model::Parent;
use verifier_core::probe_plan;
use verifier_core::scoring::SCORE_TOLERANCE;

/// L: the compound fixture, which combines double reduction legality, homolog
/// label symmetry with repeated rows, recombination across intervals, and a
/// conflict between the global and the marker-at-a-time optimum.
///
/// Each of those properties is asserted to still hold before the candidate is
/// graded on it, so the hardest fixture cannot silently become an easy one.
pub fn l_the_compound_case_is_solved() -> Outcome {
    let fixture = support::load("compound");
    let expected = &fixture.expected;

    let uses_dr = expected.paths.iter().any(|path| {
        Parent::BOTH.iter().any(|p| path.states(*p).iter().any(|s| s.is_double_reduction()))
    });
    demand!(uses_dr, "the compound fixture no longer exercises double reduction");

    let forbids_dr_somewhere = Parent::BOTH
        .iter()
        .any(|p| fixture.cross.params(*p).dr_allowed.iter().any(|allowed| !allowed));
    demand!(
        forbids_dr_somewhere,
        "the compound fixture no longer forbids double reduction at any marker"
    );

    let repeated_rows = Parent::BOTH.iter().any(|p| {
        let rows = fixture.expected.haplotypes(*p).rows();
        rows.windows(2).any(|pair| pair[0] == pair[1])
    });
    demand!(repeated_rows, "the compound fixture no longer has two identical homolog rows");

    let switches = expected
        .paths
        .iter()
        .flat_map(|path| Parent::BOTH.iter().map(move |p| path.states(*p)))
        .flat_map(|states| states.windows(2))
        .filter(|pair| pair[0].switch_count(pair[1]) > 0)
        .count();
    demand!(switches > 0, "the compound fixture no longer contains a recombination event");

    let greedy = marker_at_a_time_score(&fixture.cross);
    demand!(
        greedy + SCORE_TOLERANCE < fixture.optimum,
        "the compound fixture no longer defeats a marker-at-a-time reconstruction"
    );

    let produced = answer!("compound");
    if let Some(problem) = support::compare(&fixture, &produced) {
        fail!("compound: {problem}");
    }
    Outcome::Pass
}

/// M: the answer is computed from the cross that was supplied.
///
/// Adding the same constant to every genotype likelihood shifts every
/// reconstruction's total by the same amount, so the maximiser, the canonical
/// form and the tie ordering are all unchanged. The shifted cross is not any
/// document that ships with the task, so an answer recalled by recognising its
/// input cannot survive the substitution.
///
/// The shift is applied by the probe, from `probe_plan::LIKELIHOOD_SHIFT`, to a
/// cross the candidate never sees in its original form. A shifted job is a
/// separate candidate execution on a genuinely different input, not a second
/// reading of an answer already computed.
///
/// The same check confirms the grading itself is not vacuous: a reconstruction
/// with one inheritance state altered must be rejected.
pub fn m_answers_follow_the_supplied_cross() -> Outcome {
    // A zero shift would leave the substitution below solving the same cross
    // twice, which every fixture already satisfies. The group would then pass
    // without testing anything.
    demand!(
        probe_plan::LIKELIHOOD_SHIFT != 0.0,
        "the shifted jobs no longer change the cross, so this group is vacuous"
    );

    for stem in ["baseline", "symmetry", "compound"] {
        let fixture = support::load(stem);
        let produced = answer!(stem);
        if let Some(problem) = support::compare(&fixture, &produced) {
            fail!("{stem}: {problem}");
        }

        let from_shifted = answer!(&format!("{stem}_shifted"));
        demand!(
            from_shifted == produced,
            "{stem}: shifting every genotype likelihood by a constant changed the answer, \
             although it cannot change which reconstruction is optimal"
        );

        // The grading has to be able to say no, or none of the above means
        // anything. One altered state must be rejected.
        let mut broken = produced.clone();
        let last = fixture.cross.markers - 1;
        let current = broken.paths[0].from_a[last];
        let replacement = if current.low() == 0 && current.high() == 1 {
            verifier_core::cross_model::State::new(2, 3)
        } else {
            verifier_core::cross_model::State::new(0, 1)
        }
        .expect("both replacements name existing homologs");
        broken.paths[0].from_a[last] = replacement;

        demand!(
            support::compare(&fixture, &broken).is_some(),
            "{stem}: grading accepted a reconstruction with an altered inheritance state, \
             so it would accept a wrong answer"
        );
    }
    Outcome::Pass
}
