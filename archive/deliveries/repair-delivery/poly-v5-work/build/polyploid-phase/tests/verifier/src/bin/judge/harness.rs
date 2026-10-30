
//! Test registry and process-per-check runner.
//!
//! Each check runs in its own process, so a panic inside one check fails that
//! check rather than the whole run.
//!
//! This binary links no candidate code. The solver is compiled into `probe` and
//! nowhere else, so nothing the candidate wrote ever shares a process with the
//! certified answers, and nothing it printed is read here.
//!
//! The payload arrives on standard input and is consumed to end of file before
//! the check is dispatched. The runner trusts only this verifier-owned process's
//! exact marker and exit status; candidate output is captured elsewhere.

use std::fmt::Write as _;
use std::io::Read as _;

/// Marker prefix a passing check prints. The runner matches the whole line.
const PASS_MARKER: &str = "##PHASE-PASS";

/// What one check reports.
pub enum Outcome {
    Pass,
    Fail(String),
}

/// A named check and the function that performs it.
pub struct Check {
    pub name: &'static str,
    pub run: fn() -> Outcome,
}

/// Fails the check unless the condition holds.
macro_rules! demand {
    ($cond:expr, $($arg:tt)*) => {
        // Matched rather than negated. Most conditions here compare scores, and
        // negating a float comparison reads as though the two are always
        // comparable; spelling both arms out keeps the false branch explicit.
        match $cond {
            true => {}
            false => return crate::harness::Outcome::Fail(format!($($arg)*)),
        }
    };
}

/// Fails the check outright.
macro_rules! fail {
    ($($arg:tt)*) => {
        return crate::harness::Outcome::Fail(format!($($arg)*))
    };
}

/// The reconstruction a named probe job produced, or a failed check.
///
/// Reading an artifact is the only way a check obtains a candidate answer. There
/// is deliberately no way to ask for one that is not already in
/// `probe_plan::PLAN`, which is what keeps the number of candidate executions a
/// property of the plan rather than of how many checks want an answer.
macro_rules! answer {
    ($job:expr) => {
        match crate::support::answer($job) {
            Ok(answer) => answer,
            Err(reason) => return crate::harness::Outcome::Fail(reason),
        }
    };
}

/// Registers one check. One invocation per line, which is how the runner counts
/// the checks the suite defines without trusting the built binary.
macro_rules! check {
    ($name:ident) => {
        crate::harness::Check { name: stringify!($name), run: $name }
    };
}

/// Registers a control. Spelled differently from `check!` so that counting the
/// graded suite cannot accidentally include something that is not graded.
macro_rules! control {
    ($name:ident) => {
        crate::harness::Check { name: stringify!($name), run: $name }
    };
}

pub fn run_main(checks: &[Check], controls: &[Check]) -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("--list") => {
            // Controls are deliberately absent. The runner counts what it
            // expects to pass, and a control is not one of those.
            let mut out = String::new();
            for check in checks {
                let _ = writeln!(out, "{}", check.name);
            }
            print!("{out}");
            std::process::ExitCode::SUCCESS
        }
        Some("--run") => {
            let Some(name) = args.get(1) else {
                eprintln!("--run needs a check name");
                return std::process::ExitCode::from(2);
            };
            let Some(check) = checks.iter().chain(controls).find(|c| c.name == name) else {
                eprintln!("no such check: {name}");
                return std::process::ExitCode::from(2);
            };

            // Consumed before the check runs, so the payload is gone from the
            // process before anything reads an artifact.
            let mut raw = String::new();
            if std::io::stdin().read_to_string(&mut raw).is_err() {
                eprintln!("the run payload could not be read; refusing to report a pass");
                return std::process::ExitCode::from(2);
            }
            let payload = match crate::run_payload::parse(&raw) {
                Ok(payload) => payload,
                Err(reason) => {
                    // Fail closed. A process started by anything other than the
                    // runner has no payload, and must not report a pass.
                    eprintln!("the run payload is unusable: {reason}");
                    return std::process::ExitCode::from(2);
                }
            };
            crate::run_payload::install(payload);

            match (check.run)() {
                Outcome::Pass => {
                    println!("{PASS_MARKER} {}", check.name);
                    std::process::ExitCode::SUCCESS
                }
                Outcome::Fail(reason) => {
                    println!("FAIL {}: {reason}", check.name);
                    std::process::ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: judge --list | --run <name>");
            std::process::ExitCode::from(2)
        }
    }
}
