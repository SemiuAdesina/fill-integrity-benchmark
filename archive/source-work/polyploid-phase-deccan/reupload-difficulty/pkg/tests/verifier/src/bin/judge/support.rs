
//! Fixture loading, artifact loading, and the comparisons the checks share.
//!
//! Ground truth is a result document that the reference oracle certified before
//! the fixture was accepted into the corpus. Nothing here re-derives an optimum,
//! so no check can end up grading the candidate against the candidate.
//!
//! The cross comes off the verifier's filesystem; the certified answer does not.
//! A cross is only an input, and the candidate is meant to solve it. A certified
//! answer is delivered on standard input by the runner, after the last probe has
//! exited, and parsed before any check is dispatched. See `run_payload`.
//!
//! No check runs the candidate. Every reconstruction a check grades was produced
//! by a `probe` process, once, and reaches this binary in the same payload as the
//! certified answers. That is what fixes the cost of a graded run: the candidate
//! is executed as many times as `probe_plan::PLAN` has entries and no more,
//! however many assertions go on to read the results. It is also what removes the
//! residual exposure the stdin channel could only raise the cost of — the
//! certified answers are held by a process that links no candidate code at all.

use verifier_core::cross_input::{parse_cross, Cross};
use verifier_core::cross_model::Reconstruction;
use verifier_core::digest::canonical_digest;
use verifier_core::probe_artifact;
use verifier_core::result_output::decode;
use verifier_core::scoring::{score, scores_equal};

/// Where the runner places the verifier's own copy of the fixtures.
const DEFAULT_FIXTURE_DIR: &str = "/verifier/fixtures";

/// One fixture: the cross the candidate is given, and the certified answer.
pub struct Fixture {
    pub cross: Cross,
    pub expected: Reconstruction,
    pub optimum: f64,
    pub digest: String,
}

fn fixture_dir() -> String {
    std::env::var("PHASE_FIXTURE_DIR").unwrap_or_else(|_| DEFAULT_FIXTURE_DIR.to_string())
}

/// Loads a fixture by stem, aborting if the verifier's own data is unusable.
///
/// A missing or malformed fixture is a fault in the verifier image rather than
/// a candidate failure, so it panics instead of reporting a failed check. The
/// difference matters: a failed check is the candidate's problem, and this is
/// not.
pub fn load(stem: &str) -> Fixture {
    let dir = fixture_dir();
    let cross_path = format!("{dir}/{stem}.cross.json");

    let cross_text = std::fs::read_to_string(&cross_path)
        .unwrap_or_else(|e| panic!("verifier fixture unreadable at {cross_path}: {e}"));
    let expected_text = crate::run_payload::get().expectation(stem);

    let cross = parse_cross(&cross_text)
        .unwrap_or_else(|e| panic!("verifier fixture {stem} does not parse: {e}"));
    let decoded = decode(expected_text, cross.markers, cross.offspring)
        .unwrap_or_else(|e| panic!("verifier expectation {stem} does not decode: {e}"));

    Fixture {
        cross,
        expected: decoded.answer,
        optimum: decoded.total_log_likelihood,
        digest: decoded.canonical_digest,
    }
}

/// The reconstruction one probe produced.
///
/// The artifact arrives in the run payload, captured by the runner the moment
/// that probe exited. The error arm is a candidate failure, not a verifier
/// fault, so it is returned rather than panicked: a probe that crashed, timed
/// out, or was never reached because the run's probe budget ran out leaves no
/// artifact, and the checks that wanted it must fail with a reason in the
/// report.
pub fn answer(job: &str) -> Result<Reconstruction, String> {
    let Some(text) = crate::run_payload::get().artifact(job) else {
        return Err(format!(
            "the candidate produced no answer for job '{job}': its probe did not finish"
        ));
    };
    probe_artifact::decode(text)
        .map_err(|e| format!("the answer for job '{job}' is not a usable reconstruction: {e}"))
}

/// Difference between a produced reconstruction and the certified answer.
///
/// Returns `None` when they agree on every graded property. The message names
/// the first property that differs and never prints the expected answer, so a
/// failing run does not hand back the thing it is grading.
pub fn compare(fixture: &Fixture, produced: &Reconstruction) -> Option<String> {
    let scored = match score(&fixture.cross, produced) {
        Ok(total) => total,
        Err(e) => return Some(format!("the scorer rejects the reconstruction: {e}")),
    };

    if !scores_equal(scored, fixture.optimum) {
        return Some(format!(
            "total log likelihood {scored:.10} is not the global optimum for this cross"
        ));
    }

    if produced.haplotypes_a.rows() != fixture.expected.haplotypes_a.rows() {
        return Some("parent A haplotypes are not in the required canonical form".to_string());
    }
    if produced.haplotypes_b.rows() != fixture.expected.haplotypes_b.rows() {
        return Some("parent B haplotypes are not in the required canonical form".to_string());
    }

    for (c, (got, want)) in produced.paths.iter().zip(fixture.expected.paths.iter()).enumerate() {
        if got.from_a != want.from_a {
            return Some(format!("progeny {c} parent A inheritance path differs"));
        }
        if got.from_b != want.from_b {
            return Some(format!("progeny {c} parent B inheritance path differs"));
        }
    }

    let got_digest = canonical_digest(produced);
    if got_digest != fixture.digest {
        return Some(format!("canonical digest {got_digest} is not the required digest"));
    }

    None
}
