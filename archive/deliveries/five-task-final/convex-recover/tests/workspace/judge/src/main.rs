//! Verifier-owned judge — the sole grading authority.
//!
//! The judge never links candidate code and never runs in the same process as
//! it. It reads only the raw functional values a probe recorded, holds the
//! expected properties itself, and decides correctness from its own comparisons.
//! Candidate stdout and stderr are not opened here at all, the probe's exit
//! status is not consulted, and the record codec has no field in which a
//! candidate could assert a verdict. Nothing the graded process emits can
//! therefore contribute to a pass.
mod expected;

use convex_model::cases;
use convex_model::record::{decode, MAX_RECORD_BYTES};
use convex_model::RawResult;
use std::fmt::Write as _;

/// Read one probe record. A missing, symlinked, irregular, oversized or
/// malformed record is a failure, never a skip.
fn read_record(dir: &str, id: &str) -> Result<Option<RawResult>, String> {
    let path = std::path::Path::new(dir).join(id);
    let meta = std::fs::symlink_metadata(&path).map_err(|_| "no result recorded".to_string())?;
    if meta.file_type().is_symlink() {
        return Err("result path is a symlink".into());
    }
    if !meta.file_type().is_file() {
        return Err("result path is not a regular file".into());
    }
    if meta.len() as usize > MAX_RECORD_BYTES {
        return Err("result is implausibly large".into());
    }
    let raw = std::fs::read_to_string(&path).map_err(|_| "result is not readable text".to_string())?;
    decode(&raw)
}

/// Grade one case: read the recorded values, then apply the verifier's own
/// expectation for that case.
fn grade_case(dir: &str, id: &str, out: &mut String) -> bool {
    let got = match read_record(dir, id) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(out, "FAIL {id} {e}");
            return false;
        }
    };
    match expected::grade(id, got.as_ref()) {
        Ok(()) => {
            let _ = writeln!(out, "PASS {id}");
            true
        }
        Err(e) => {
            let _ = writeln!(out, "FAIL {id} {e}");
            false
        }
    }
}

/// Grade the whole expected case population. The total is the judge's own
/// count of the cases it decided to evaluate, not anything read from a record.
fn grade_all(dir: &str) -> (String, usize, usize) {
    let mut out = String::new();
    let mut bad = 0;
    for id in cases::CANDIDATE_CASES {
        if !grade_case(dir, id, &mut out) {
            bad += 1
        }
    }
    let total = cases::CANDIDATE_CASES.len();
    let _ = writeln!(out, "TOTAL {total} FAILED {bad}");
    (out, total, bad)
}

/// Required negative control, determined entirely inside the verifier.
///
/// The judge stages a full set of known-correct records itself, perturbs exactly
/// one functional value in exactly one of them, and grades the result. No
/// candidate code runs and no candidate output is consulted. Success means the
/// injected defect was observed: exactly one case failed, it was the perturbed
/// one, and every other case still passed. Anything else means the pipeline
/// cannot see failure and must not be trusted to award reward.
fn selftest(dir: &str) -> i32 {
    let target = "compound";
    if std::fs::create_dir_all(dir).is_err() {
        eprintln!("control: cannot stage results");
        return 2;
    }
    for id in cases::CANDIDATE_CASES {
        let mut staged = expected::canonical(id);
        if id == &target {
            // Perturb one coordinate off the integer lattice. The shape stays
            // legal, so only the property this case actually asserts can catch
            // it, which is precisely what the control must demonstrate.
            let Some(r) = staged.as_mut() else {
                eprintln!("control: no reconstruction to perturb");
                return 2;
            };
            let Some(v) = r.vertices.first_mut() else {
                eprintln!("control: empty vertex list");
                return 2;
            };
            v.xyz[0].n = v.xyz[0].n.wrapping_mul(2);
            v.xyz[0].d = 2;
        }
        if std::fs::write(
            std::path::Path::new(dir).join(id),
            convex_model::record::encode(staged.as_ref()),
        )
        .is_err()
        {
            eprintln!("control: cannot write staged result");
            return 2;
        }
    }
    let (out, total, bad) = grade_all(dir);
    print!("{out}");
    let saw_target = out.contains(&format!("FAIL {target} "));
    if total == cases::CANDIDATE_CASES.len() && bad == 1 && saw_target {
        println!("CONTROL ok: injected defect observed");
        0
    } else {
        println!("CONTROL compromised: injected defect not observed cleanly");
        1
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() == 3 && a[1] == "--selftest" {
        std::process::exit(selftest(&a[2]))
    }
    if a.len() != 4 || a[1] != "--grade" {
        eprintln!("usage: judge --grade <results-dir> <verdict-file> | judge --selftest <dir>");
        std::process::exit(2)
    }
    let (dir, verdict) = (&a[2], &a[3]);
    let (out, _total, bad) = grade_all(dir);
    print!("{out}");
    if std::fs::write(verdict, &out).is_err() {
        std::process::exit(2)
    }
    std::process::exit(if bad == 0 { 0 } else { 1 })
}
