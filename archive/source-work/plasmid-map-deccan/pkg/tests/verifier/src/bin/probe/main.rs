
//! One candidate execution.
//!
//! This binary is the only place the candidate's solver is compiled. It reads
//! one job's problem from the runner's fixture directory, applies the job's
//! transform, calls the candidate's `solve` once, and writes the map it was
//! given to the path the runner named. It grades nothing and holds no
//! certified answer; its standard output and exit status carry no grading
//! authority.
//!
//! The frozen model is re-exported at this crate's root so candidate source
//! written against `crate::input` and friends compiles here unchanged.

pub use verifier_core::digest;
pub use verifier_core::fasta;
pub use verifier_core::input;
pub use verifier_core::json;
pub use verifier_core::map_model;
pub use verifier_core::result_output;
pub use verifier_core::sites;

mod solver;

use verifier_core::input::parse_problem;
use verifier_core::probe_plan;
use verifier_core::result_output::encode;

const DEFAULT_FIXTURE_DIR: &str = "/verifier/fixtures";

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
    let dir = std::env::var("PLASMID_FIXTURE_DIR").unwrap_or_else(|_| DEFAULT_FIXTURE_DIR.to_string());
    let stem_dir = format!("{dir}/{}", job.stem);
    let texts = (|| -> Result<(String, String, String), String> {
        Ok((read(&format!("{stem_dir}/backbone.fasta"))?, read(&format!("{stem_dir}/enzymes.json"))?, read(&format!("{stem_dir}/digests.json"))?))
    })();
    let (fasta, enzymes, digests) = match texts {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };
    let mut problem = match parse_problem(&fasta, &enzymes, &digests) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("fixture {} does not parse: {e}", job.stem);
            return std::process::ExitCode::from(2);
        }
    };
    job.transform.apply(&mut problem);

    // The one call into the candidate's code that this process makes.
    let answer = solver::solve(&problem);

    let encoded = encode(&answer);
    if let Err(e) = std::fs::write(out_path, encoded.as_bytes()) {
        eprintln!("could not write the artifact for {id}: {e}");
        return std::process::ExitCode::from(2);
    }
    std::process::ExitCode::SUCCESS
}
