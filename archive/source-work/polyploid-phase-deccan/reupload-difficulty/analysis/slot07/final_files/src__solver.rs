
//! The search.
//!
//! **This is the solver entry point you are asked to write.**

use crate::cross_input::Cross;
use crate::cross_model::{
    Haplotypes, Parent, ProgenyPath, Reconstruction, State, ALL_STATES, HOMOLOGS,
};

mod local;
mod model;

use model::{Model, NS};

/// Finds the highest scoring reconstruction for a cross.
pub fn solve(cross: &Cross) -> Reconstruction {
    let model = Model::new(cross);
    let (z, _) = local::search_local(&model, 64, 0x9E3779B97F4A7C15);
    assemble(&model, &z)
}

/// All permutations of the four homolog indices, in ascending image order.
pub fn homolog_permutations() -> Vec<[u8; HOMOLOGS]> {
    let mut out = Vec::with_capacity(24);
    for a in 0..HOMOLOGS as u8 {
        for b in 0..HOMOLOGS as u8 {
            for c in 0..HOMOLOGS as u8 {
                for d in 0..HOMOLOGS as u8 {
                    let perm = [a, b, c, d];
                    let mut seen = [false; HOMOLOGS];
                    if perm.iter().all(|x| {
                        let fresh = !seen[*x as usize];
                        seen[*x as usize] = true;
                        fresh
                    }) {
                        out.push(perm);
                    }
                }
            }
        }
    }
    out
}

/// Splits a marker's joint column choice into one choice per parent.
pub fn split_z(model: &Model, m: usize, z: usize) -> (usize, usize) {
    let nb = model.ncol[1][m];
    (z / nb, z % nb)
}

/// Parental haplotypes named by an assignment.
pub fn haplotypes_of(model: &Model, z: &[usize], parent: Parent) -> Haplotypes {
    let p = parent.index();
    let mut haps = Haplotypes::zeros(model.markers);
    for m in 0..model.markers {
        let (ia, ib) = split_z(model, m, z[m]);
        let column = if p == 0 {
            model.columns[0][m][ia]
        } else {
            model.columns[1][m][ib]
        };
        for h in 0..HOMOLOGS {
            haps.set(h, m, column[h]);
        }
    }
    haps
}

/// A reconstruction from an assignment, taking each progeny's best path.
pub fn assemble(model: &Model, z: &[usize]) -> Reconstruction {
    let haplotypes_a = haplotypes_of(model, z, Parent::A);
    let haplotypes_b = haplotypes_of(model, z, Parent::B);
    let mut paths = Vec::with_capacity(model.offspring);
    for c in 0..model.offspring {
        let (_, joint) = model.viterbi_path(c, z);
        let mut from_a = Vec::with_capacity(model.markers);
        let mut from_b = Vec::with_capacity(model.markers);
        for j in joint {
            from_a.push(ALL_STATES[j / NS]);
            from_b.push(ALL_STATES[j % NS]);
        }
        paths.push(ProgenyPath { from_a, from_b });
    }
    Reconstruction { haplotypes_a, haplotypes_b, paths }
}

/// Rebuilds a `State` from its two homologs.
pub fn state_of(a: u8, b: u8) -> State {
    State::new(a, b).expect("homolog indices in range")
}

#[cfg(test)]
mod dev {
    use super::*;
    use crate::scoring::score;

    fn cross_from_file(path: &str) -> Cross {
        let text = std::fs::read_to_string(path).expect("read cross");
        crate::cross_input::parse_cross(&text).expect("valid cross")
    }

    #[test]
    fn search_matches_enumeration() {
        for path in ["/tmp/free4.json", "/tmp/free5.json"] {
            let Ok(text) = std::fs::read_to_string(path) else { continue };
            let cross = crate::cross_input::parse_cross(&text).expect("valid cross");
            let model = Model::new(&cross);
            let optimum = dev::brute_force(&model);
            let (z, value) = local::search_local(&model, 64, 0x2545F4914F6CDD1D);
            let mut search = search::Search::new(&model, 1e-8);
            let complete = search.run(value, z, 5_000_000);
            println!(
                "{path}: brute {optimum:.6} search {:.6} complete {complete} nodes {}",
                search.best, search.nodes
            );
            assert!(complete, "search ran out of budget on {path}");
            assert!(
                (search.best - optimum).abs() < 1e-9,
                "search found {} but enumeration says {optimum}",
                search.best
            );
        }
    }

    #[test]
    fn search_on_the_fixture() {
        let cross = cross_from_file("/app/cross.json");
        let model = Model::new(&cross);
        let (z, value) = local::search_local(&model, 256, 0x2545F4914F6CDD1D);
        let start = std::time::Instant::now();
        let mut search = search::Search::new(&model, 1e-8);
        let complete = search.run(value, z, 3_000_000);
        println!(
            "seed {value:.6} best {:.6} complete {complete} nodes {} ties {} in {:?}",
            search.best,
            search.nodes,
            search.best_z.len(),
            start.elapsed()
        );
    }

    #[test]
    fn lagrange_trace() {
        let cross = cross_from_file("/app/cross.json");
        let model = Model::new(&cross);
        let (_, lower) = local::search_local(&model, 256, 0x2545F4914F6CDD1D);
        let mut lag = lagrange::Lagrange::new(&model);
        let mut picks = vec![vec![0usize; model.markers]; model.offspring];
        for round in 0..=600 {
            let value = lag.evaluate(&model, &mut picks);
            if round % 40 == 0 {
                println!("round {round:4} L = {value:.6}");
            }
            let gap = value - lower;
            let mut norm = 0.0f64;
            let mut moves = Vec::new();
            for m in 0..model.markers {
                let mut low = f64::INFINITY;
                let mut shared = 0usize;
                for v in 0..model.zcount[m] {
                    let mut sum = 0.0;
                    for c in 0..model.offspring {
                        sum += lag.lambda[c][m][v];
                    }
                    if sum < low {
                        low = sum;
                        shared = v;
                    }
                }
                for c in 0..model.offspring {
                    if picks[c][m] != shared {
                        norm += 2.0;
                        moves.push((c, m, picks[c][m], shared));
                    }
                }
            }
            if norm == 0.0 {
                println!("agree at round {round}");
                break;
            }
            let step = 0.05 * gap / norm;
            for (c, m, mine, shared) in moves {
                lag.lambda[c][m][mine] -= step;
                lag.lambda[c][m][shared] += step;
            }
        }
    }

    #[test]
    fn viterbi_matches_the_scorer() {
        let cross = cross_from_file("/app/cross.json");
        let model = Model::new(&cross);
        let (z, value) = local::search_local(&model, 32, 0x2545F4914F6CDD1D);
        let answer = assemble(&model, &z);
        let total = score(&cross, &answer).expect("structurally valid");
        assert!(
            (total - value).abs() < 1e-9,
            "scorer says {total} but the model says {value}"
        );
        println!("local best = {value}");
    }
}
