
//! The graded suite.
//!
//! Thirteen checks, groups A to M. Each names one behaviour that
//! `instruction.md` and the three specification documents state publicly. The
//! runner executes them one process at a time and grades on how many passed.
//!
//! | Group | What it validates |
//! |---|---|
//! | A | `/app/result.json` exists, decodes, and holds the optimum and its own digest |
//! | B | Each parent's four homolog rows match `parent_dosage` at every marker |
//! | C | Inheritance states are ordered pairs naming homologs that exist |
//! | D | No double reduction where the cross forbids it |
//! | E | Double reduction is used where the optimum contains it |
//! | F | Answers are the canonical representative of their relabelling class |
//! | G | Canonicalisation remaps paths with the rows and is idempotent |
//! | H | Every fixture reaches the exact global optimum |
//! | I | A marker-at-a-time reconstruction falls short of that optimum |
//! | J | Recombination between markers is part of the objective |
//! | K | Repeated runs agree, and ties resolve to the documented representative |
//! | L | The compound fixture is solved, and still combines all four difficulties |
//! | M | Answers follow the supplied cross, and grading can reject a wrong one |
//!
//! No expected value appears in that table, in any check name, or in any failure
//! message. The certified answers arrive on standard input, sent by the runner
//! from its root-only workspace after the last probe has exited, and are parsed
//! into memory before a check runs; nothing certified is read from the filesystem.
//!
//! This binary links no candidate code. Every reconstruction it grades was
//! produced by a `probe` process and reaches it as an artifact, so a check
//! cannot cause a candidate execution however many answers it inspects.
//!
//! The registry below is also the count. `tests/test.sh` reads the number of
//! `check!` invocations out of this file's source rather than asking the built
//! binary how many tests it has, so a candidate that replaces the binary with
//! one reporting a shorter suite does not shorten the suite.

#[macro_use]
mod harness;

mod canonical_check;
mod greedy_baseline;
mod groups_document;
mod groups_hard;
mod groups_legality;
mod groups_optimum;
mod run_payload;
mod support;

use groups_document::{
    a_required_output_document_holds_the_optimum, b_haplotypes_respect_parent_dosage,
    c_inheritance_states_are_ordered_pairs,
};
use groups_hard::{l_the_compound_case_is_solved, m_answers_follow_the_supplied_cross};
use groups_legality::{
    d_double_reduction_never_appears_where_forbidden,
    e_double_reduction_is_used_where_it_is_optimal, f_answers_are_in_canonical_form,
    g_canonicalisation_remaps_paths_and_is_idempotent,
};
use groups_optimum::{
    h_every_fixture_reaches_the_global_optimum, i_marker_at_a_time_reconstruction_falls_short,
    j_recombination_between_markers_is_accounted_for,
    k_ties_resolve_to_the_documented_representative,
};
use harness::Outcome;

/// A check that must fail.
///
/// The runner executes it and requires a non-zero exit with no pass marker. It
/// is the evidence that a failing check is actually detected, which nothing
/// else in the run establishes: a suite that reported success unconditionally
/// would look identical from the outside.
fn negative_control_must_fail() -> Outcome {
    Outcome::Fail("this check exists to fail".to_string())
}

fn main() -> std::process::ExitCode {
    let checks = [
        // A: the agent left a decodable document carrying both a score and a
        // digest of its own content. Catches a missing, truncated or
        // self-inconsistent result file before any semantic check runs.
        check!(a_required_output_document_holds_the_optimum),
        // B: every parent's four homolog rows agree with the declared
        // parent_dosage at every marker. Catches rows invented independently of
        // the cross the agent was given.
        check!(b_haplotypes_respect_parent_dosage),
        // C: each inheritance state is an ordered pair of homologs that exist.
        // Catches an out-of-range homolog index and a pair recorded in the wrong
        // order, both of which make a path unreadable.
        check!(c_inheritance_states_are_ordered_pairs),
        // D: no double reduction appears where the cross forbids it. Catches a
        // solver that buys score with an illegal inheritance event.
        check!(d_double_reduction_never_appears_where_forbidden),
        // E: double reduction is used where the optimum contains it. The
        // converse of D, and the reason a solver cannot pass D by refusing
        // double reduction outright.
        check!(e_double_reduction_is_used_where_it_is_optimal),
        // F: the answer is the canonical representative of its relabelling
        // class. Catches a correctly scoring reconstruction reported under an
        // arbitrary homolog labelling.
        check!(f_answers_are_in_canonical_form),
        // G: canonicalisation remaps the paths along with the rows, and applying
        // it twice changes nothing. Catches relabelled rows whose paths still
        // point at pre-remap homologs, and a non-idempotent normaliser.
        check!(g_canonicalisation_remaps_paths_and_is_idempotent),
        // H: every fixture reaches the exact global optimum, not a near miss.
        // The primary correctness check; a heuristic that is usually right fails
        // here on the fixture that punishes it.
        check!(h_every_fixture_reaches_the_global_optimum),
        // I: a marker-at-a-time reconstruction scores strictly worse than the
        // optimum. This grades the fixture, not the agent: if a greedy sweep
        // could match the optimum, H would not be evidence of a real search.
        check!(i_marker_at_a_time_reconstruction_falls_short),
        // J: recombination between adjacent markers is part of the objective.
        // Catches a solver that optimises each marker independently and so never
        // pays for the transitions between them.
        check!(j_recombination_between_markers_is_accounted_for),
        // K: two runs on the same input agree, and a tie resolves to the
        // documented representative. Catches nondeterminism leaking in from
        // container iteration order and an arbitrarily broken tie.
        check!(k_ties_resolve_to_the_documented_representative),
        // L: the compound fixture is solved, and it still genuinely combines all
        // four difficulties. Catches a fixture that has degenerated into an easy
        // case as much as a solver that fails it.
        check!(l_the_compound_case_is_solved),
        // M: the answer tracks the cross actually supplied, and grading rejects a
        // wrong one. The load-bearing check: it is what a memorised, constant or
        // replayed answer cannot survive.
        check!(m_answers_follow_the_supplied_cross),
    ];
    let controls = [control!(negative_control_must_fail)];

    harness::run_main(&checks, &controls)
}
