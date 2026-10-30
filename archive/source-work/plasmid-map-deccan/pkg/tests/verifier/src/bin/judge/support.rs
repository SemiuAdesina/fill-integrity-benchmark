
//! Fixture loading, artifact loading, and the comparisons the checks share.
//!
//! Ground truth is the map the author-side exact searches certified before
//! the fixture entered the corpus, together with the bands the true
//! construct produced in the held-out digests. Nothing here re-derives an
//! optimum, so no check grades the candidate against the candidate.
//!
//! The problem comes off the verifier's filesystem; the certified answer does
//! not. It is delivered on standard input by the runner after the last probe
//! has exited, and parsed before any check is dispatched (`run_payload`).

use std::collections::BTreeMap;

use verifier_core::digest;
use verifier_core::input::{parse_problem, Band, Problem};
use verifier_core::json::{self, Json};
use verifier_core::map_model::{evaluate, internal_count, validate, PlasmidMap};
use verifier_core::probe_plan::{self, Transform};
use verifier_core::result_output::decode;

const DEFAULT_FIXTURE_DIR: &str = "/verifier/fixtures";

/// One fixture: the problem the candidate is given and what was certified.
pub struct Fixture {
    pub problem: Problem,
    pub expected: PlasmidMap,
    pub internal: u32,
    /// The bands the true construct produced in each held-out digest.
    pub held_out_truth: BTreeMap<String, Vec<Band>>,
    /// How many maps tie with the answer on count and deviation (1: none).
    pub ties: u64,
}

fn fixture_dir() -> String {
    std::env::var("PLASMID_FIXTURE_DIR").unwrap_or_else(|_| DEFAULT_FIXTURE_DIR.to_string())
}

fn integer(v: &Json, what: &str) -> u64 {
    let n = v.as_number().unwrap_or_else(|| panic!("certified {what} is not a number"));
    n as u64
}

/// Loads a fixture by stem, aborting if the verifier's own data is unusable.
/// A missing or malformed fixture is a fault in the verifier image rather
/// than a candidate failure, so it panics instead of reporting a failed check.
pub fn load(stem: &str) -> Fixture {
    let dir = format!("{}/{stem}", fixture_dir());
    let read = |name: &str| std::fs::read_to_string(format!("{dir}/{name}")).unwrap_or_else(|e| panic!("verifier fixture {stem}/{name} unreadable: {e}"));
    let problem = parse_problem(&read("backbone.fasta"), &read("enzymes.json"), &read("digests.json"))
        .unwrap_or_else(|e| panic!("verifier fixture {stem} does not parse: {e}"));
    let text = crate::run_payload::get().expectation(stem);
    let doc = json::parse(text).unwrap_or_else(|e| panic!("certified answer for {stem} is not JSON: {e}"));
    let root = doc.as_object().unwrap_or_else(|| panic!("certified answer for {stem} is not an object"));
    let map_doc = root.get("map").unwrap_or_else(|| panic!("certified answer for {stem} has no map"));
    let expected = decode(&json::write(map_doc)).unwrap_or_else(|e| panic!("certified map for {stem} does not decode: {e}"));
    validate(&problem, &expected).unwrap_or_else(|e| panic!("certified map for {stem} is not valid under the frozen model: {e}"));
    let deviation = evaluate(&problem, &expected).unwrap_or_else(|e| panic!("certified map for {stem} does not reproduce its gels: {e}"));
    let internal = internal_count(&expected);
    assert_eq!(internal as u64, integer(root.get("internal").expect("internal"), "internal"), "certified count for {stem} disagrees with the frozen model");
    assert_eq!(deviation, integer(root.get("deviation").expect("deviation"), "deviation"), "certified deviation for {stem} disagrees with the frozen model");
    let _ = deviation;
    let mut held_out_truth = BTreeMap::new();
    for (id, bands) in root.get("held_out_truth").and_then(|v| v.as_object()).unwrap_or_else(|| panic!("certified answer for {stem} has no held-out bands")) {
        let list = bands
            .as_array()
            .unwrap_or_else(|| panic!("held-out bands of {stem} malformed"))
            .iter()
            .map(|b| {
                let o = b.as_object().expect("band object");
                Band { length: integer(o.get("length").expect("length"), "length") as u32, multiplicity: integer(o.get("multiplicity").expect("multiplicity"), "multiplicity") as u32 }
            })
            .collect();
        held_out_truth.insert(id.clone(), list);
    }
    let ties = root.get("ties_at_count_and_deviation").map(|v| integer(v, "ties")).unwrap_or(0);
    Fixture { problem, expected, internal, held_out_truth, ties }
}

/// The map one probe produced.
///
/// The error arm is a candidate failure, so it is returned rather than
/// panicked: a probe that crashed, timed out, or was never reached because
/// the run's probe budget ran out leaves no artifact.
pub fn answer(job: &str) -> Result<PlasmidMap, String> {
    let Some(text) = crate::run_payload::get().artifact(job) else {
        return Err(format!("the candidate produced no answer for job '{job}': its probe did not finish"));
    };
    decode(text).map_err(|e| format!("the answer for job '{job}' is not a usable map document: {e}"))
}

/// The problem a job actually handed to the candidate.
pub fn problem_of(job: &str) -> Problem {
    let j = probe_plan::find(job).unwrap_or_else(|| panic!("no job {job}"));
    let mut problem = load(j.stem).problem;
    j.transform.apply(&mut problem);
    problem
}

/// The certified map as the candidate should report it for a transformed job.
pub fn expected_of(job: &str) -> PlasmidMap {
    let j = probe_plan::find(job).unwrap_or_else(|| panic!("no job {job}"));
    let fixture = load(j.stem);
    match j.transform {
        Transform::AsSupplied => fixture.expected,
        Transform::Rotate(r) => rotate(&fixture.problem, &fixture.expected, r),
    }
}

/// The certified map read from a backbone origin `r` bases later: the
/// construct is the same molecule, so every position moves by the same
/// amount, which is `r` when the new origin lies before the cloning site and
/// `L + r` when it lies at or after it.
pub fn rotate(problem: &Problem, map: &PlasmidMap, r: usize) -> PlasmidMap {
    let v = problem.v();
    let r = r % v;
    let l = map.insert_length;
    let n = v + l;
    let shift = if r <= map.cloning_site { r } else { l + r };
    let cloning_site = (map.cloning_site + v - r) % v;
    let mut cuts = BTreeMap::new();
    for (enzyme, positions) in &map.cuts {
        let mut moved: Vec<usize> = positions.iter().map(|&p| (p + n - shift % n) % n).collect();
        moved.sort_unstable();
        cuts.insert(enzyme.clone(), moved);
    }
    let held_out = verifier_core::map_model::predictions(problem, l, &cuts);
    PlasmidMap { cloning_site, insert_length: l, cuts, held_out }
}

/// Difference between a produced map and the certified answer for a job.
/// Returns `None` when they agree on every graded property. The message
/// never prints the certified answer.
pub fn compare(job: &str, produced: &PlasmidMap) -> Option<String> {
    let problem = problem_of(job);
    if let Err(e) = validate(&problem, produced) {
        return Some(format!("the map is not valid: {e}"));
    }
    let dev = match evaluate(&problem, produced) {
        Ok(dev) => dev,
        Err(e) => return Some(format!("the map does not reproduce the gels: {e}")),
    };
    let expected = expected_of(job);
    let want_dev = evaluate(&problem, &expected).expect("certified map reproduces the gels");
    let got = internal_count(produced);
    let want = internal_count(&expected);
    if got != want {
        return Some(format!("the map has {got} insert-region sites; the least map has a different number"));
    }
    if dev != want_dev {
        return Some(format!("the map's total deviation {dev} is not the least attainable at this site count"));
    }
    if produced.cloning_site != expected.cloning_site || produced.insert_length != expected.insert_length || produced.cuts != expected.cuts {
        return Some("the map ties the least map on count and deviation but is not the required one under the position order".to_string());
    }
    None
}

/// Validity, reproduction, and the certified count and deviation, without
/// requiring the exact positions: what a transformed job is graded on.
pub fn compare_objective(job: &str, produced: &PlasmidMap) -> Option<String> {
    let problem = problem_of(job);
    if let Err(e) = validate(&problem, produced) {
        return Some(format!("the map is not valid for the supplied backbone: {e}"));
    }
    let dev = match evaluate(&problem, produced) {
        Ok(dev) => dev,
        Err(e) => return Some(format!("the map does not reproduce the gels: {e}")),
    };
    let expected = expected_of(job);
    let want_dev = evaluate(&problem, &expected).expect("certified map reproduces the gels");
    if internal_count(produced) != internal_count(&expected) {
        return Some("the map has a different number of insert-region sites from the least map".to_string());
    }
    if dev != want_dev {
        return Some(format!("the map's total deviation {dev} is not the least attainable"));
    }
    None
}

/// Whether a map's held-out predictions match the true construct's bands.
pub fn held_out_ok(fixture: &Fixture, produced: &PlasmidMap) -> Result<(), String> {
    for (id, bands) in &fixture.held_out_truth {
        let Some(predicted) = produced.held_out.get(id) else {
            return Err(format!("no prediction for held-out digest {id}"));
        };
        let expanded = digest::expand(predicted);
        if digest::deviation(&expanded, bands, fixture.problem.error_percent).is_none() {
            return Err(format!("the predicted bands of held-out digest {id} do not match the construct's within tolerance"));
        }
    }
    Ok(())
}
