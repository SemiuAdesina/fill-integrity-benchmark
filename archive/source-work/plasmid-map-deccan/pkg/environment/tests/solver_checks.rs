
//! Solver checks on constructs small enough to enumerate.
//!
//! Each case builds a toy backbone and insert, scans the true construct,
//! simulates the gels with a fixed sizing error, then enumerates every valid
//! map with at most one insert-region site per enzyme and takes the least
//! under the model's order. `solve` must return exactly that map. Agreement
//! here says nothing about constructs that cannot be enumerated.

use std::collections::BTreeMap;

use plasmid_map::digest::{group, predicted};
use plasmid_map::input::{parse_problem, Problem};
use plasmid_map::map_model::{key, key_of, predictions, PlasmidMap};
use plasmid_map::sites::{required_positions, scan};
use plasmid_map::solver::solve;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const PANEL: [(&str, &str, usize); 3] = [("EcoRI", "GAATTC", 1), ("HindIII", "AAGCTT", 1), ("PstI", "CTGCAG", 5)];

fn random_seq(rng: &mut Lcg, len: usize) -> Vec<u8> {
    (0..len).map(|_| b"ACGT"[rng.below(4) as usize]).collect()
}

fn plant(seq: &mut [u8], at: usize, site: &[u8]) {
    for (k, b) in site.iter().enumerate() {
        let n = seq.len();
        seq[(at + k) % n] = *b;
    }
}

/// Builds one enumerable case; returns the problem text triple.
fn case(seed: u64, error_percent: u32, detection_limit: u32) -> (String, String, String) {
    let mut rng = Lcg(seed);
    let v = 140 + rng.below(60) as usize;
    let mut backbone = random_seq(&mut rng, v);
    for (_, site, _) in PANEL {
        plant(&mut backbone, rng.below(v as u64) as usize, site.as_bytes());
        if rng.below(2) == 0 {
            plant(&mut backbone, rng.below(v as u64) as usize, site.as_bytes());
        }
    }
    let l = 8 + rng.below(20) as usize;
    let mut insert = random_seq(&mut rng, l);
    for (_, site, _) in PANEL {
        if rng.below(2) == 0 && l > 8 {
            plant(&mut insert, rng.below((l - 6) as u64) as usize, site.as_bytes());
        }
    }
    let c = 20 + rng.below((v - 40) as u64) as usize;
    let mut construct = backbone[..c].to_vec();
    construct.extend_from_slice(&insert);
    construct.extend_from_slice(&backbone[c..]);
    let n = v + l;
    let mut cuts: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (name, site, offset) in PANEL {
        let mut positions: Vec<usize> = scan(&construct, site.as_bytes(), offset).iter().map(|s| s.cut).collect();
        positions.sort_unstable();
        positions.dedup();
        cuts.insert(name.to_string(), positions);
    }
    let fasta = format!(">toy\n{}\n", std::str::from_utf8(&backbone).unwrap());
    let enzymes = format!(
        r#"{{"format_version":1,"enzymes":[{}],"cloning_sites":[{{"enzyme":"toy","position":{c}}}]}}"#,
        PANEL.iter().map(|(n, s, o)| format!(r#"{{"name":"{n}","site":"{s}","cut_offset":{o}}}"#)).collect::<Vec<_>>().join(",")
    );
    let mut observed = Vec::new();
    let mut sets: Vec<Vec<&str>> = PANEL.iter().map(|(n, _, _)| vec![*n]).collect();
    sets.push(vec!["EcoRI", "PstI"]);
    for set in &sets {
        let names: Vec<String> = set.iter().map(|s| s.to_string()).collect();
        let bands = group(&predicted(&cuts, &names, n, detection_limit));
        let items: Vec<String> = bands
            .iter()
            .map(|b| {
                // sizing error: shrink or stretch by up to half the tolerance
                let tol = plasmid_map::digest::tolerance(b.length, error_percent) as i64;
                let shift = (rng.below((tol / 2 + 1) as u64) as i64) * if rng.below(2) == 0 { 1 } else { -1 };
                format!(r#"{{"length":{},"multiplicity":{}}}"#, (b.length as i64 + shift).max(1), b.multiplicity)
            })
            .collect();
        observed.push(format!(r#"{{"id":"{}","enzymes":[{}],"bands":[{}]}}"#, names.join("+"), names.iter().map(|x| format!("\"{x}\"")).collect::<Vec<_>>().join(","), items.join(",")));
    }
    let digests = format!(
        r#"{{"format_version":1,"error_percent":{error_percent},"detection_limit":{detection_limit},"insert_length":{{"min":8,"max":27}},"max_internal_sites":1,"observed":[{}],"held_out":[{{"id":"HindIII+PstI","enzymes":["HindIII","PstI"]}}]}}"#,
        observed.join(",")
    );
    (fasta, enzymes, digests)
}

/// The least valid map that reproduces every gel, by exhaustive enumeration.
fn least_map(problem: &Problem) -> Option<PlasmidMap> {
    let mut best: Option<(plasmid_map::map_model::Key, PlasmidMap)> = None;
    for site in &problem.cloning_sites {
        let c = site.position;
        for l in problem.insert_min..=problem.insert_max {
            let required: Vec<Vec<usize>> = problem.enzymes.iter().map(|e| required_positions(&problem.backbone.seq, e, c, l)).collect();
            // at most one insert-region site per enzyme: none, or any position in [c, c + l]
            let options: Vec<Vec<Option<usize>>> = (0..problem.enzymes.len()).map(|_| std::iter::once(None).chain((c..=c + l).map(Some)).collect()).collect();
            let mut idx = vec![0usize; options.len()];
            loop {
                let mut cuts = BTreeMap::new();
                for (i, e) in problem.enzymes.iter().enumerate() {
                    let mut positions = required[i].clone();
                    if let Some(q) = options[i][idx[i]] {
                        positions.push(q);
                    }
                    positions.sort_unstable();
                    cuts.insert(e.name.clone(), positions);
                }
                let held_out = predictions(problem, l, &cuts);
                let map = PlasmidMap { cloning_site: c, insert_length: l, cuts, held_out };
                if let Ok(k) = key_of(problem, &map) {
                    if best.as_ref().map_or(true, |(bk, _)| k < *bk) {
                        best = Some((k, map));
                    }
                }
                let mut i = 0;
                while i < idx.len() {
                    idx[i] += 1;
                    if idx[i] < options[i].len() {
                        break;
                    }
                    idx[i] = 0;
                    i += 1;
                }
                if i == idx.len() {
                    break;
                }
            }
        }
    }
    best.map(|(_, m)| m)
}

fn run_case(seed: u64, error_percent: u32, detection_limit: u32) {
    let (fasta, enzymes, digests) = case(seed, error_percent, detection_limit);
    let Ok(problem) = parse_problem(&fasta, &enzymes, &digests) else {
        return; // this seed produced an input the format rejects (a torn only-site); skip
    };
    let Some(expected) = least_map(&problem) else {
        return; // no map reproduces the perturbed gels; graded problems always have one
    };
    let produced = solve(&problem);
    let want = key_of(&problem, &expected).expect("the least map is valid and reproduces the gels");
    match key_of(&problem, &produced) {
        Ok(k) => assert_eq!(k, want, "seed {seed}: the map returned is not the least map (count {}, deviation {} required)", want.internal, want.deviation),
        Err(e) => panic!("seed {seed}: the map returned is rejected: {e} (a map with count {} and deviation {} exists)", want.internal, want.deviation),
    }
    let _ = key(&produced, 0);
}

#[test]
fn small_constructs_yield_the_least_map_at_five_percent() {
    for seed in 1..=6 {
        run_case(seed, 5, 4);
    }
}

#[test]
fn small_constructs_yield_the_least_map_at_two_percent() {
    for seed in 7..=12 {
        run_case(seed, 2, 6);
    }
}

#[test]
fn small_constructs_with_a_detection_limit_that_hides_fragments() {
    for seed in 13..=18 {
        run_case(seed, 5, 12);
    }
}
