
//! The graded suite.
//!
//! Thirteen checks, A to M. Each names one behaviour that `instruction.md`
//! and the three specification documents state publicly. The runner executes
//! them one process at a time and grades on how many passed.
//!
//! | Check | What it validates |
//! |---|---|
//! | A | every job's document decodes and is physically valid |
//! | B | vector-region positions are the backbone's surviving sites |
//! | C | every observed digest is reproduced |
//! | D | every fixture yields the least map exactly |
//! | E | fewer insert-region sites come before a closer fit |
//! | F | fragments below the detection limit are unseen |
//! | G | bands carry multiplicity |
//! | H | the cloning site is chosen among all listed |
//! | I | ties resolve by position order, and repeated runs agree |
//! | J | held-out digests are predicted within tolerance |
//! | K | the large constructs are recovered within budget |
//! | L | one percent sizing error is handled |
//! | M | answers follow the supplied backbone |
//!
//! No expected value appears in that table, in any check name, or in any
//! failure message. The certified answers arrive on standard input, sent by
//! the runner from its root-only workspace after the last probe has exited.
//!
//! This binary links no candidate code. Every map it grades was produced by a
//! `probe` process and reaches it as an artifact. The registry below is also
//! the count: `tests/test.sh` reads the number of `check!` invocations out of
//! this file's source rather than asking the built binary.

#[macro_use]
mod harness;

mod checks;
mod run_payload;
mod support;

use checks::*;
use harness::Outcome;

/// A check that must fail. The runner requires a non-zero exit with no pass
/// marker; it is the evidence that a failing check is actually detected.
fn negative_control_must_fail() -> Outcome {
    Outcome::Fail("this check exists to fail".to_string())
}

fn main() -> std::process::ExitCode {
    let checks = [
        check!(a_map_documents_decode_and_are_valid),
        check!(b_vector_region_positions_match_the_backbone),
        check!(c_every_observed_digest_is_reproduced),
        check!(d_every_fixture_yields_the_least_map),
        check!(e_fewer_insert_region_sites_come_first),
        check!(f_fragments_below_the_detection_limit_are_unseen),
        check!(g_bands_carry_multiplicity),
        check!(h_the_cloning_site_is_chosen_among_all_listed),
        check!(i_ties_resolve_by_position_order),
        check!(j_held_out_digests_are_predicted_within_tolerance),
        check!(k_large_constructs_are_recovered_within_budget),
        check!(l_low_sizing_error_is_handled),
        check!(m_answers_follow_the_supplied_backbone),
    ];
    let controls = [control!(negative_control_must_fail)];
    harness::run_main(&checks, &controls)
}
