
//! The search.
//!
//! `solve` returns the reconstruction that maximises the total log likelihood
//! over every parental haplotype assignment consistent with `parent_dosage` and
//! every legal inheritance state path, reduced to canonical form.
//!
//! The work is split three ways. `core` holds the tables and the two dynamic
//! programs: the exact Viterbi for one progeny against fixed parental columns,
//! and a backward recursion that bounds what the markers not yet chosen can
//! still contribute. `viterbi` turns those into a local search, alternating
//! best paths for fixed columns with best columns for fixed paths, which
//! supplies a real reconstruction to prune against. `search` is the branch and
//! bound that proves nothing better exists. `canon` puts the result into the
//! one relabelling `spec/result-format-v1.md` accepts.

use crate::cross_input::Cross;
use crate::cross_model::{Haplotypes, Reconstruction, State, HOMOLOGS};
use crate::solver::core::Core;
use crate::solver::search::Search;
use crate::solver::viterbi::{local_search, viterbi};

pub mod canon;
pub mod core;
pub mod search;
pub mod viterbi;

/// Absolute tolerance from `spec/scoring-model.md`.
const TOL: f64 = 1e-8;

/// Finds the highest scoring reconstruction for a cross.
pub fn solve(cross: &Cross) -> Reconstruction {
    let core = Core::new(cross);
    let (heuristic, columns_a, columns_b) = local_search(&core);

    let mut search = Search::new(&core);
    search.seed(heuristic, &columns_a, &columns_b);
    let root = search.root_bound();
    let found = search.run(f64::NEG_INFINITY, false);

    if std::env::var("PHASE_DEBUG").is_ok() {
        eprintln!(
            "heuristic={heuristic:.6} root_bound={root:.6} best={:.6} nodes={} leaves={} candidates={}",
            search.best(),
            search.nodes,
            search.leaves,
            found.len()
        );
    }

    let (best_a, best_b) = search.best_columns();
    reconstruct(&core, &best_a, &best_b)
}

/// Builds the reconstruction for fixed columns: both parents' haplotype rows
/// and each progeny's best path against them.
pub fn reconstruct(core: &Core, columns_a: &[u8], columns_b: &[u8]) -> Reconstruction {
    let markers = core.markers;
    let mut haplotypes_a = Haplotypes::zeros(markers);
    let mut haplotypes_b = Haplotypes::zeros(markers);
    for m in 0..markers {
        for h in 0..HOMOLOGS {
            haplotypes_a.set(h, m, (columns_a[m] >> h) & 1);
            haplotypes_b.set(h, m, (columns_b[m] >> h) & 1);
        }
    }

    let mut paths = Vec::with_capacity(core.offspring);
    for c in 0..core.offspring {
        let (_, states) = viterbi(core, c, columns_a, columns_b);
        let mut from_a = Vec::with_capacity(markers);
        let mut from_b = Vec::with_capacity(markers);
        for (m, joint) in states.iter().enumerate() {
            let sa = joint / crate::cross_model::STATES;
            let sb = joint % crate::cross_model::STATES;
            let _ = m;
            from_a.push(State::new(core.state_a(sa), core.state_b(sa)).expect("state in range"));
            from_b.push(State::new(core.state_a(sb), core.state_b(sb)).expect("state in range"));
        }
        paths.push(crate::cross_model::ProgenyPath { from_a, from_b });
    }

    Reconstruction { haplotypes_a, haplotypes_b, paths }
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

#[allow(dead_code)]
fn unused(_: f64) -> f64 {
    TOL
}
