
//! Reads a cross, runs the search, writes a result.
//!
//! With no arguments it reads `/app/cross.json` and writes `/app/result.json`.
//! Both paths can be overridden positionally, which is convenient while
//! iterating on extra instances of your own.

use std::process::ExitCode;

use polyploid_phase::cross_input::parse_cross;
use polyploid_phase::result_output::encode;
use polyploid_phase::scoring::score;
use polyploid_phase::solver::solve;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let input = args.first().map(String::as_str).unwrap_or("/app/cross.json");
    let output = args.get(1).map(String::as_str).unwrap_or("/app/result.json");

    let text = match std::fs::read_to_string(input) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("cannot read {input}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let cross = match parse_cross(&text) {
        Ok(cross) => cross,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let answer = solve(&cross);
    let total = match score(&cross, &answer) {
        Ok(total) => total,
        Err(e) => {
            eprintln!("the search produced a reconstruction the scorer rejects: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = std::fs::write(output, encode(&answer, total)) {
        eprintln!("cannot write {output}: {e}");
        return ExitCode::FAILURE;
    }

    println!("markers={} offspring={}", cross.markers, cross.offspring);
    println!("total_log_likelihood={total:.10}");
    println!("wrote {output}");
    ExitCode::SUCCESS
}
