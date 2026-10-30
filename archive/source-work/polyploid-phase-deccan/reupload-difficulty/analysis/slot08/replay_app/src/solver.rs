
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
use crate::cross_model::{Haplotypes, Parent, Reconstruction, State, HOMOLOGS};
use crate::cross_model::STATES;
use crate::solver::canon::{
    canonical_permutation, canonicalize, distinct_key,
};
use crate::solver::core::{Core, SA, SB};
use crate::solver::lexmin::{lexmin_path, state_order};
use crate::solver::search::Search;
use crate::solver::viterbi::{local_search, viterbi};

pub mod canon;
pub mod core;
pub mod lexmin;
pub mod search;
pub mod viterbi;

/// Absolute tolerance from `spec/scoring-model.md`.
const TOL: f64 = 1e-8;
/// How far a single progeny's path may fall below its own best and still count
/// as tied. Wide enough to absorb floating point noise in a long sum, far
/// narrower than the tolerance itself.
const SLACK: f64 = 1e-11;

/// Finds the highest scoring reconstruction for a cross.
///
/// The branch and bound proves the optimum; what is left is the choice among
/// reconstructions that tie, which `spec/result-format-v1.md` settles by
/// parent A's rows, parent B's, then the written state sequence.
pub fn solve(cross: &Cross) -> Reconstruction {
    let core = Core::new(cross);
    let (heuristic, columns_a, columns_b) = local_search(&core);

    let mut search = Search::new(&core);
    search.seed(heuristic, &columns_a, &columns_b);
    search.run(f64::NEG_INFINITY, false);
    let optimum = search.best();

    if std::env::var("PHASE_DEBUG").is_ok() {
        eprintln!(
            "heuristic={heuristic:.6} root={:.6} optimum={optimum:.6} nodes={} leaves={}",
            search.root_bound(),
            search.nodes,
            search.leaves
        );
    }

    let candidates = search.run(optimum - TOL, true);
    if std::env::var("PHASE_DEBUG").is_ok() {
        eprintln!("optimum={optimum:.6} tied_assignments={}", candidates.len());
    }

    let mut best: Option<(Vec<u8>, Reconstruction)> = None;
    for candidate in &candidates {
        if candidate.score < optimum - TOL {
            continue;
        }
        let answer = best_answer(&core, &candidate.columns_a, &candidate.columns_b);
        let key = distinct_key(&answer);
        if best.as_ref().is_none_or(|(held, _)| key < *held) {
            best = Some((key, answer));
        }
    }
    best.expect("the optimum is reachable, so at least one assignment attains it").1
}

/// The smallest reconstruction for one pair of parental assignments.
///
/// Equal homolog rows leave the relabelling underdetermined, and the
/// relabelling changes which paths are smallest, so each admissible relabelling
/// is tried and the one the canonical form would pick is kept.
fn best_answer(core: &Core, columns_a: &[u8], columns_b: &[u8]) -> Reconstruction {
    let markers = core.markers;
    let mut haplotypes_a = Haplotypes::zeros(markers);
    let mut haplotypes_b = Haplotypes::zeros(markers);
    for m in 0..markers {
        for h in 0..HOMOLOGS {
            haplotypes_a.set(h, m, (columns_a[m] >> h) & 1);
            haplotypes_b.set(h, m, (columns_b[m] >> h) & 1);
        }
    }

    let mut best: Option<(Vec<u8>, Reconstruction)> = None;
    for relabel_a in relabellings(&haplotypes_a) {
        for relabel_b in relabellings(&haplotypes_b) {
            let order_a = state_order(&relabel_a);
            let order_b = state_order(&relabel_b);
            let mut paths = Vec::with_capacity(core.offspring);
            for c in 0..core.offspring {
                let (_, states) =
                    lexmin_path(core, c, columns_a, columns_b, &order_a, &order_b, SLACK);
                paths.push(states_to_path(&states));
            }
            let tentative = Reconstruction {
                haplotypes_a: haplotypes_a.clone(),
                haplotypes_b: haplotypes_b.clone(),
                paths,
            };
            if canonical_permutation(&tentative, Parent::A) != relabel_a
                || canonical_permutation(&tentative, Parent::B) != relabel_b
            {
                continue;
            }
            let answer = canonicalize(&tentative);
            let key = distinct_key(&answer);
            if best.as_ref().is_none_or(|(held, _)| key < *held) {
                best = Some((key, answer));
            }
        }
    }
    best.expect("the identity relabelling always survives the check").1
}

/// Turns joint state indices into one progeny's two state sequences.
fn states_to_path(states: &[usize]) -> crate::cross_model::ProgenyPath {
    let mut from_a = Vec::with_capacity(states.len());
    let mut from_b = Vec::with_capacity(states.len());
    for &joint in states {
        let sa = joint / STATES;
        let sb = joint % STATES;
        from_a.push(State::new(SA[sa] as u8, SB[sa] as u8).expect("state in range"));
        from_b.push(State::new(SA[sb] as u8, SB[sb] as u8).expect("state in range"));
    }
    crate::cross_model::ProgenyPath { from_a, from_b }
}

/// Relabellings that leave a parent's rows untouched: the permutations within
/// each block of identical rows.
fn relabellings(haplotypes: &Haplotypes) -> Vec<[u8; HOMOLOGS]> {
    let rows = haplotypes.rows();
    let mut blocks: Vec<Vec<u8>> = Vec::new();
    for h in 0..HOMOLOGS {
        match blocks.last_mut() {
            Some(block) if rows[usize::from(*block.last().unwrap())] == rows[h] => block.push(h as u8),
            _ => blocks.push(vec![h as u8]),
        }
    }

    let mut out = vec![[0, 1, 2, 3]];
    for block in &blocks {
        if block.len() == 1 {
            continue;
        }
        let mut grown = Vec::new();
        for base in &out {
            for permutation in permutations_of(block) {
                let mut next = *base;
                for (slot, homolog) in block.iter().enumerate() {
                    next[usize::from(*homolog)] = permutation[slot];
                }
                grown.push(next);
            }
        }
        out = grown;
    }
    out
}

fn permutations_of(items: &[u8]) -> Vec<Vec<u8>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let mut rest = items.to_vec();
        rest.remove(index);
        for tail in permutations_of(&rest) {
            let mut whole = vec![*item];
            whole.extend_from_slice(&tail);
            out.push(whole);
        }
    }
    out
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
            let sa = joint / STATES;
            let sb = joint % STATES;
            let _ = m;
            from_a.push(State::new(SA[sa] as u8, SB[sa] as u8).expect("state in range"));
            from_b.push(State::new(SA[sb] as u8, SB[sb] as u8).expect("state in range"));
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
