
//! One candidate execution.
//!
//! This binary is the only place the candidate's solver is compiled. It solves
//! exactly one job from [`probe_plan`](verifier_core::probe_plan), writes the
//! reconstruction it was given to the file the runner named, and exits. It
//! grades nothing, it is told nothing it could replay, and the judge does not
//! read anything it printed.
//!
//! Three properties of this binary are what the split is for.
//!
//! It holds no ground truth. The certified answers are deleted from every
//! filesystem before the first probe starts and are delivered only to the judge,
//! which links no candidate code, so a solver that reads everything it can reach
//! finds nothing to replay.
//!
//! It holds no run token. The pass marker the runner matches carries a token the
//! judge echoes; a probe never receives it, so nothing the candidate prints can
//! be mistaken for a passing check. Standard output is captured to a log and has
//! no part in grading.
//!
//! It runs once per job. The runner drives the plan; a semantic assertion in the
//! judge reads an artifact and cannot cause another execution. That is the whole
//! of the runtime fix: the number of candidate executions is fixed by
//! `probe_plan::PLAN`, not by how many assertions want an answer.
//!
//! The frozen core is re-exported at this crate's root so candidate source
//! written against `crate::cross_model` and friends compiles here unchanged, as
//! it does in the agent image. `probe_plan` and `probe_artifact` are deliberately
//! not among them: the solver being graded cannot see the plan it is part of.

pub use verifier_core::cross_input;
pub use verifier_core::cross_model;
pub use verifier_core::digest;
pub use verifier_core::json;
pub use verifier_core::result_output;
pub use verifier_core::scoring;

mod solver;

use verifier_core::cross_input::parse_cross;
use verifier_core::probe_artifact;
use verifier_core::probe_plan;

/// Where the runner places the verifier's own copy of the fixtures.
const DEFAULT_FIXTURE_DIR: &str = "/verifier/fixtures";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // The runner cross-checks this list against the number of `job!` lines in
    // `probe_plan.rs`, the same way it cross-checks the judge's `--list` against
    // the number of `check!` lines. A candidate that replaced the built binary
    // with one reporting a shorter plan does not shorten the plan.
    if args.first().map(String::as_str) == Some("--list") {
        for job in probe_plan::PLAN {
            println!("{}", job.id);
        }
        return std::process::ExitCode::SUCCESS;
    }

    let (Some(id), Some(out_path)) = (args.first(), args.get(1)) else {
        eprintln!("usage: probe --list | probe <job-id> <output-path>");
        return std::process::ExitCode::from(2);
    };

    let Some(job) = probe_plan::find(id) else {
        eprintln!("no such job: {id}");
        return std::process::ExitCode::from(2);
    };

    let dir = std::env::var("PHASE_FIXTURE_DIR").unwrap_or_else(|_| DEFAULT_FIXTURE_DIR.to_string());
    let cross_path = format!("{dir}/{}.cross.json", job.stem);
    let text = match std::fs::read_to_string(&cross_path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("fixture unreadable at {cross_path}: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let mut cross = match parse_cross(&text) {
        Ok(cross) => cross,
        Err(e) => {
            eprintln!("fixture {} does not parse: {e}", job.stem);
            return std::process::ExitCode::from(2);
        }
    };
    job.transform.apply(&mut cross);

    // The one call into the candidate's code that this process makes.
    let answer = solver::solve(&cross);

    // Written whole, then checked. A truncated artifact would reach the judge as
    // a decode failure, which grades as a wrong answer rather than as the write
    // error it actually was.
    let encoded = probe_artifact::encode(&answer);
    if let Err(e) = std::fs::write(out_path, encoded.as_bytes()) {
        eprintln!("could not write the artifact for {id}: {e}");
        return std::process::ExitCode::from(2);
    }
    match std::fs::metadata(out_path) {
        Ok(meta) if meta.len() as usize == encoded.len() => std::process::ExitCode::SUCCESS,
        Ok(meta) => {
            eprintln!("the artifact for {id} is {} bytes, expected {}", meta.len(), encoded.len());
            std::process::ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("the artifact for {id} is unreadable after writing: {e}");
            std::process::ExitCode::from(2)
        }
    }
}
