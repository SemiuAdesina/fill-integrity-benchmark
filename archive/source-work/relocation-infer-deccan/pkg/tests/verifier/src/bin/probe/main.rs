//! Candidate-linked probe: `probe <job> <artifact-path>`.
//!
//! Loads the job's corpus from `$RELOC_FIXTURE_DIR/<fixture>/corpus` with the
//! verifier's own loader, runs the candidate's `recover`, and writes the result
//! document to the artifact path. It compares nothing and decides nothing;
//! its stdout, stderr and exit status carry no grading authority.

pub use verifier_core::{bits, corpus, elf, json, program};


mod recover;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: probe <job> <artifact>");
        std::process::exit(2);
    }
    let fixture = match verifier_core::plan::fixture_of(&args[1]) {
        Some(f) => f,
        None => { eprintln!("unknown job {}", args[1]); std::process::exit(2); }
    };
    let root = std::env::var("RELOC_FIXTURE_DIR").unwrap_or_else(|_| "/tests/fixtures".into());
    let dir = std::path::Path::new(&root).join(fixture).join("corpus");
    let corpus = match corpus::Corpus::load(&dir) {
        Ok(c) => c,
        Err(e) => { eprintln!("probe: cannot load corpus {}: {}", dir.display(), e); std::process::exit(1); }
    };
    match recover::recover(&corpus) {
        Ok(result) => {
            if let Err(e) = std::fs::write(&args[2], result.to_json()) {
                eprintln!("probe: cannot write artifact: {}", e);
                std::process::exit(1);
            }
        }
        Err(e) => { eprintln!("probe: recovery failed: {}", e); std::process::exit(1); }
    }
}
