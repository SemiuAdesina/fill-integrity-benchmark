
//! Reads the problem, runs the search, writes the map.
//!
//! With no arguments it reads `/app/backbone.fasta`, `/app/enzymes.json` and
//! `/app/digests.json` and writes `/app/map.json`. A directory can be given
//! as the first argument and an output path as the second, which is convenient
//! while iterating on extra instances of your own.

use std::process::ExitCode;

use plasmid_map::input::parse_problem;
use plasmid_map::map_model::{evaluate, internal_count, validate};
use plasmid_map::result_output::encode;
use plasmid_map::solver::solve;

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = args.first().map(String::as_str).unwrap_or("/app");
    let output = args.get(1).cloned().unwrap_or_else(|| format!("{dir}/map.json"));

    let texts = (|| -> Result<(String, String, String), String> {
        Ok((read(&format!("{dir}/backbone.fasta"))?, read(&format!("{dir}/enzymes.json"))?, read(&format!("{dir}/digests.json"))?))
    })();
    let (fasta, enzymes, digests) = match texts {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let problem = match parse_problem(&fasta, &enzymes, &digests) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let map = solve(&problem);
    if let Err(e) = validate(&problem, &map) {
        eprintln!("the search produced a map the model rejects: {e}");
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::write(&output, encode(&map)) {
        eprintln!("cannot write {output}: {e}");
        return ExitCode::FAILURE;
    }

    println!("backbone={} enzymes={} observed={} held_out={}", problem.v(), problem.enzymes.len(), problem.observed.len(), problem.held_out.len());
    println!("cloning_site={} insert_length={} insert_region_sites={}", map.cloning_site, map.insert_length, internal_count(&map));
    match evaluate(&problem, &map) {
        Ok(dev) => println!("every observed digest reproduced, deviation={dev}"),
        Err(e) => println!("{e}"),
    }
    println!("wrote {output}");
    ExitCode::SUCCESS
}
