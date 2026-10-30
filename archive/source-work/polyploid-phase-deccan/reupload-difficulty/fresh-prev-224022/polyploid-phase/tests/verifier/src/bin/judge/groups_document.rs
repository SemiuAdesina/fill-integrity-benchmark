
//! Groups A, B and C: the output document and its structural rules.
//!
//! A grades the document the agent left at `/app/result.json`. B and C grade the
//! reconstructions the probes produced, one artifact per fixture, read rather
//! than recomputed.

use crate::harness::Outcome;
use crate::support;
use verifier_core::cross_model::{Parent, HOMOLOGS};
use verifier_core::probe_plan::ALL_FIXTURES;
use verifier_core::result_output::decode;
use verifier_core::scoring::{score, scores_equal};

fn app_dir() -> String {
    std::env::var("PHASE_APP_DIR").unwrap_or_else(|_| "/app".to_string())
}

/// A: `/app/result.json` exists, decodes under the version 1 format, and holds
/// the global optimum for the cross the task shipped at `/app/cross.json`.
pub fn a_required_output_document_holds_the_optimum() -> Outcome {
    // Graded against the verifier's own copy of the primary cross. Editing
    // `/app/cross.json` to something easier changes nothing that is scored.
    let fixture = support::load("baseline");
    let path = format!("{}/result.json", app_dir());

    let Ok(text) = std::fs::read_to_string(&path) else {
        fail!("{path} is missing or unreadable");
    };

    let decoded = match decode(&text, fixture.cross.markers, fixture.cross.offspring) {
        Ok(d) => d,
        Err(e) => fail!("{path} is not a valid result document: {e}"),
    };

    if let Some(problem) = support::compare(&fixture, &decoded.answer) {
        fail!("{path} does not hold the required answer: {problem}");
    }

    let scored = match score(&fixture.cross, &decoded.answer) {
        Ok(total) => total,
        Err(e) => fail!("{path} cannot be scored: {e}"),
    };
    demand!(
        scores_equal(decoded.total_log_likelihood, scored),
        "the document reports {:.10} but its own reconstruction scores {scored:.10}",
        decoded.total_log_likelihood
    );
    demand!(
        decoded.canonical_digest == fixture.digest,
        "the document's canonical_digest is not the digest of the required answer"
    );

    Outcome::Pass
}

/// B: each parent's four homolog rows carry exactly the alternate allele count
/// that `parent_dosage` fixes at every marker.
pub fn b_haplotypes_respect_parent_dosage() -> Outcome {
    for stem in ALL_FIXTURES {
        let fixture = support::load(stem);
        let produced = answer!(stem);

        for parent in Parent::BOTH {
            let haps = produced.haplotypes(parent);
            demand!(
                haps.markers() == fixture.cross.markers,
                "{stem}: parent {} has {} markers, expected {}",
                parent.label(),
                haps.markers(),
                fixture.cross.markers
            );
            let required = &fixture.cross.params(parent).dosage;
            // Bounded by the cross's marker count, matching the scorer: the
            // cross is the authority on the shape, not the dosage vector.
            #[allow(clippy::needless_range_loop)]
            for m in 0..fixture.cross.markers {
                for h in 0..HOMOLOGS {
                    demand!(
                        haps.row(h)[m] <= 1,
                        "{stem}: parent {} homolog {h} marker {m} is not a 0/1 allele",
                        parent.label()
                    );
                }
                demand!(
                    haps.dosage_at(m) == required[m],
                    "{stem}: parent {} marker {m} carries {} alternate alleles, \
                     parent_dosage requires {}",
                    parent.label(),
                    haps.dosage_at(m),
                    required[m]
                );
            }
        }
    }
    Outcome::Pass
}

/// C: every inheritance state is an unordered pair inside `0 <= a <= b < 4`,
/// and every progeny has one state per marker for each parent.
pub fn c_inheritance_states_are_ordered_pairs() -> Outcome {
    for stem in ALL_FIXTURES {
        let fixture = support::load(stem);
        let produced = answer!(stem);

        demand!(
            produced.paths.len() == fixture.cross.offspring,
            "{stem}: {} paths, expected {}",
            produced.paths.len(),
            fixture.cross.offspring
        );

        for (c, path) in produced.paths.iter().enumerate() {
            for parent in Parent::BOTH {
                let states = path.states(parent);
                demand!(
                    states.len() == fixture.cross.markers,
                    "{stem}: progeny {c} parent {} has {} states, expected {}",
                    parent.label(),
                    states.len(),
                    fixture.cross.markers
                );
                for (m, state) in states.iter().enumerate() {
                    demand!(
                        state.low() <= state.high(),
                        "{stem}: progeny {c} parent {} marker {m} is written ({}, {}), \
                         which is not in a <= b order",
                        parent.label(),
                        state.low(),
                        state.high()
                    );
                    demand!(
                        state.high() < HOMOLOGS as u8,
                        "{stem}: progeny {c} parent {} marker {m} names homolog {}, \
                         which does not exist",
                        parent.label(),
                        state.high()
                    );
                }
            }
        }
    }
    Outcome::Pass
}
