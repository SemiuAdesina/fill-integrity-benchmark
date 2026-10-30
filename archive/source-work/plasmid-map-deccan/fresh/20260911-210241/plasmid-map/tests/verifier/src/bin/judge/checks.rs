
//! The graded checks. Each names one behaviour the instruction and the
//! specification state publicly, and each reads artifacts only.

use crate::harness::Outcome;
use crate::support::{self, compare, held_out_ok, load, problem_of};
use verifier_core::map_model::{evaluate, internal_count, validate};
use verifier_core::probe_plan::{ALL_FIXTURES, PLAN};

fn all_supplied() -> Vec<&'static str> {
    ALL_FIXTURES.to_vec()
}

/// A: every job left a document that decodes and is physically valid for the
/// problem it was given.
pub fn a_map_documents_decode_and_are_valid() -> Outcome {
    for job in PLAN {
        let produced = answer!(job.id);
        let problem = problem_of(job.id);
        if let Err(e) = validate(&problem, &produced) {
            fail!("{}: {e}", job.id);
        }
    }
    Outcome::Pass
}

/// B: outside the insert region the map lists exactly the backbone's
/// surviving sites, with windows torn by the insertion dropped and cuts at
/// the junctions counted as insert-region sites.
pub fn b_vector_region_positions_match_the_backbone() -> Outcome {
    for stem in ["h_junction", "public", "h_cloning_sites"] {
        let produced = answer!(stem);
        let problem = problem_of(stem);
        if let Err(e) = validate(&problem, &produced) {
            fail!("{stem}: {e}");
        }
        if let Some(problem) = compare(stem, &produced) {
            fail!("{stem}: {problem}");
        }
    }
    Outcome::Pass
}

/// C: every observed digest is reproduced under the model's matching rule.
pub fn c_every_observed_digest_is_reproduced() -> Outcome {
    for stem in all_supplied() {
        let produced = answer!(stem);
        let problem = problem_of(stem);
        if let Err(e) = validate(&problem, &produced) {
            fail!("{stem}: {e}");
        }
        if let Err(e) = evaluate(&problem, &produced) {
            fail!("{stem}: {e}");
        }
    }
    Outcome::Pass
}

/// D: every fixture yields the certified least map exactly.
pub fn d_every_fixture_yields_the_least_map() -> Outcome {
    for stem in all_supplied() {
        let produced = answer!(stem);
        if let Some(problem) = compare(stem, &produced) {
            fail!("{stem}: {problem}");
        }
    }
    Outcome::Pass
}

/// E: fewer insert-region sites come before a closer fit. The verifier's own
/// fixture must still have this property before the candidate is graded.
pub fn e_fewer_insert_region_sites_come_first() -> Outcome {
    let fixture = load("h_parsimony");
    let produced = answer!("h_parsimony");
    demand!(
        internal_count(&produced) == fixture.internal,
        "h_parsimony: the map has {} insert-region sites; a map with {} reproduces every gel",
        internal_count(&produced),
        fixture.internal
    );
    if let Some(problem) = compare("h_parsimony", &produced) {
        fail!("h_parsimony: {problem}");
    }
    Outcome::Pass
}

/// F: fragments below the detection limit are absent from the gel, so band
/// counts and band sums understate the construct.
pub fn f_fragments_below_the_detection_limit_are_unseen() -> Outcome {
    for stem in ["h_sub_detection", "h_three_sites"] {
        let produced = answer!(stem);
        if let Some(problem) = compare(stem, &produced) {
            fail!("{stem}: {problem}");
        }
    }
    Outcome::Pass
}

/// G: an observed band carries as many fragments as its multiplicity says.
pub fn g_bands_carry_multiplicity() -> Outcome {
    let produced = answer!("h_comigration");
    if let Some(problem) = compare("h_comigration", &produced) {
        fail!("h_comigration: {problem}");
    }
    Outcome::Pass
}

/// H: the cloning site is chosen among every listed site.
pub fn h_the_cloning_site_is_chosen_among_all_listed() -> Outcome {
    let fixture = load("h_cloning_sites");
    let produced = answer!("h_cloning_sites");
    demand!(fixture.problem.cloning_sites.len() >= 4, "h_cloning_sites: the verifier's own fixture no longer lists four cloning sites");
    if let Some(problem) = compare("h_cloning_sites", &produced) {
        fail!("h_cloning_sites: {problem}");
    }
    Outcome::Pass
}

/// I: when maps tie on count and deviation, the position order decides, and
/// repeated runs agree.
pub fn i_ties_resolve_by_position_order() -> Outcome {
    let fixture = load("h_ties");
    demand!(fixture.ties >= 2, "h_ties: the verifier's own fixture no longer carries a tie on count and deviation");
    let produced = answer!("h_ties");
    let again = answer!("h_ties_again");
    demand!(produced == again, "h_ties: two runs on the same problem returned different maps");
    if let Some(problem) = compare("h_ties", &produced) {
        fail!("h_ties: {problem}");
    }
    Outcome::Pass
}

/// J: held-out digests are predicted within tolerance of what the construct
/// actually produced.
pub fn j_held_out_digests_are_predicted_within_tolerance() -> Outcome {
    for stem in all_supplied() {
        let fixture = load(stem);
        let produced = answer!(stem);
        if let Err(e) = held_out_ok(&fixture, &produced) {
            fail!("{stem}: {e}");
        }
    }
    Outcome::Pass
}

/// K: the large constructs are recovered within the probe budget.
pub fn k_large_constructs_are_recovered_within_budget() -> Outcome {
    for stem in ["s_large_a", "s_large_b", "s_large_c"] {
        let fixture = load(stem);
        demand!(fixture.internal >= 7, "{stem}: the verifier's own fixture no longer has seven insert-region sites");
        let produced = answer!(stem);
        if let Some(problem) = compare(stem, &produced) {
            fail!("{stem}: {problem}");
        }
    }
    Outcome::Pass
}

/// L: one percent sizing error is handled with the same rule.
pub fn l_low_sizing_error_is_handled() -> Outcome {
    let fixture = load("h_low_error");
    demand!(fixture.problem.error_percent == 1, "h_low_error: the verifier's own fixture is not at one percent");
    let produced = answer!("h_low_error");
    if let Some(problem) = compare("h_low_error", &produced) {
        fail!("h_low_error: {problem}");
    }
    Outcome::Pass
}

/// M: answers follow the supplied backbone: the same construct read from
/// another origin is still mapped, at the certified site count and
/// deviation, and repeated runs agree. Position order may break ties
/// differently after rotation, so the rotated answer is graded on
/// validity, reproduction, count and deviation rather than exact positions.
pub fn m_answers_follow_the_supplied_backbone() -> Outcome {
    for job in ["public_rotated", "h_low_error_rotated", "h_three_sites_rotated"] {
        let produced = answer!(job);
        if let Some(problem) = support::compare_objective(job, &produced) {
            fail!("{job}: {problem}");
        }
    }
    for (a, b) in [("public", "public_again"), ("s_large_a", "s_large_a_again")] {
        let first = answer!(a);
        let second = answer!(b);
        demand!(first == second, "{a}: two runs on the same problem returned different maps");
    }
    Outcome::Pass
}
