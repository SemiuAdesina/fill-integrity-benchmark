//! `reloc-recover [--corpus DIR] [--out FILE]`
//!
//! Loads the corpus (default `/app/corpus`), runs `recover`, and writes the
//! result document (default `/app/relocations.json`). Frozen.

use reloc_recover::corpus::Corpus;
use reloc_recover::recover::recover;
use std::path::PathBuf;

fn main() {
    let mut corpus_dir = PathBuf::from("/app/corpus");
    let mut out = PathBuf::from("/app/relocations.json");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--corpus" if i + 1 < args.len() => { corpus_dir = PathBuf::from(&args[i + 1]); i += 2; }
            "--out" if i + 1 < args.len() => { out = PathBuf::from(&args[i + 1]); i += 2; }
            other => {
                eprintln!("usage: reloc-recover [--corpus DIR] [--out FILE] (unexpected argument '{}')", other);
                std::process::exit(2);
            }
        }
    }
    let corpus = match Corpus::load(&corpus_dir) {
        Ok(c) => c,
        Err(e) => { eprintln!("reloc-recover: cannot load corpus {}: {}", corpus_dir.display(), e); std::process::exit(1); }
    };
    let result = match recover(&corpus) {
        Ok(r) => r,
        Err(e) => { eprintln!("reloc-recover: recovery failed: {}", e); std::process::exit(1); }
    };
    if let Err(e) = std::fs::write(&out, result.to_json()) {
        eprintln!("reloc-recover: cannot write {}: {}", out.display(), e);
        std::process::exit(1);
    }
    println!("wrote {} ({} programs, {} predictions)", out.display(), result.programs.len(), result.predictions.len());
}
