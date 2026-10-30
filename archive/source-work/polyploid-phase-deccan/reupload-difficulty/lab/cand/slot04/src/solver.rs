
//! The search.
//!
//! `solve` returns the reconstruction that maximises the total log likelihood
//! over every parental haplotype assignment consistent with `parent_dosage` and
//! every legal inheritance state path, reduced to canonical form as
//! `spec/result-format-v1.md` describes.
//!
//! The objective splits cleanly in two. With the parental haplotypes fixed the
//! progeny are independent and each one is a Viterbi pass over the hundred
//! joint inheritance states. With the paths fixed the markers decouple and each
//! marker's allele columns can be chosen on its own.
//!
//! Three steps use that split. `search` alternates the two halves inside
//! restarts and perturbations, and reaches a good assignment. `exact` then
//! branches over the column sequence in marker order, carrying one forward
//! vector per progeny, and either proves the assignment optimal or improves it
//! as far as its budget allows. `tiebreak` settles which of the equally scoring
//! reconstructions to report: the smallest in the order the result format
//! defines, over both the assignments the branch and bound found and the paths
//! each of them admits. `canonical` then relabels the winner.

use crate::cross_input::Cross;
use crate::cross_model::{
    Haplotypes, Parent, ProgenyPath, Reconstruction, ALL_STATES, HOMOLOGS,
};

pub mod bound;
pub mod canonical;
pub mod core;
pub mod exact;
pub mod search;
pub mod selftest;
pub mod tiebreak;

use crate::solver::canonical::{canonicalize, distinct_key};
use crate::solver::core::Tables;

/// Work the branch and bound may do, in nodes times progeny, before it stops and
/// reports the best assignment found so far. A fixed count, not a clock, so that
/// repeated runs agree exactly: a node costs time proportional to the number of
/// progeny, so dividing by it keeps the worst case near a minute either way.
const NODE_WORK: u64 = 8_000_000;
/// How many relabellings are offered to each candidate before the smallest
/// signature is settled for.
const FRAME_BUDGET: usize = 64;
/// Two totals this close count as the same score.
const SAME_SCORE: f64 = 1e-8;
/// How many equally scoring assignments are compared when a cross is degenerate
/// enough to have more.
const CANDIDATE_CAP: usize = 256;
/// Seeds for the iterated local search. Its result varies by several nats with
/// the seed on a hard cross, so several are tried and the best kept.
const SEEDS: [u64; 6] = [
    0x9e3779b97f4a7c15,
    0xbf58476d1ce4e5b9,
    0x94d049bb133111eb,
    0xd1b54a32d192ed03,
    0x1e7492d0e6fa1b47,
    0x8c7f2a51b3d0e649,
];

/// Finds the highest scoring reconstruction for a cross.
///
/// The returned reconstruction is structurally valid, globally optimal, and in
/// canonical form.
pub fn solve(cross: &Cross) -> Reconstruction {
    let tables = Tables::build(cross);
    let candidates = optimal_columns(&tables);
    let mut best: Option<(Vec<u8>, Reconstruction)> = None;
    for (columns_a, columns_b) in &candidates {
        for answer in relabellings(&tables, columns_a, columns_b, candidates.len()) {
            let key = distinct_key(&answer);
            if best.as_ref().map_or(true, |(held, _)| &key < held) {
                best = Some((key, answer));
            }
        }
    }
    best.expect("every cross has at least one admissible assignment").1
}

/// Column assignments that reach the optimum.
///
/// The iterated local search gives a lower bound, and the branch and bound then
/// raises it to the optimum and records every assignment it finds within
/// `1e-8` of that optimum.
fn optimal_columns(tables: &Tables) -> Vec<(Vec<usize>, Vec<usize>)> {
    let mut seeded = f64::NEG_INFINITY;
    let mut columns_a: Vec<usize> = vec![0; tables.markers];
    let mut columns_b: Vec<usize> = vec![0; tables.markers];
    for &seed in &SEEDS {
        let (value, trial_a, trial_b) = search::search(tables, 24, 24, seed);
        if value > seeded {
            seeded = value;
            columns_a = trial_a;
            columns_b = trial_b;
        }
    }
    // The branch and bound prunes against this, so it has to be a score the
    // assignment really attains rather than one the search merely reported.
    seeded = tables.evaluate(&columns_a, &columns_b).min(seeded);
    let mut branch = exact::Branch::new(tables, seeded - SAME_SCORE, &columns_a, &columns_b);
    branch.slack = SAME_SCORE;
    branch.run(NODE_WORK / tables.offspring.max(1) as u64);

    let mut found: Vec<(Vec<usize>, Vec<usize>)> = vec![(branch.best_a.clone(), branch.best_b.clone())];
    let mut ties: Vec<(Vec<usize>, Vec<usize>)> = branch
        .ties
        .iter()
        .map(|tie| (tie.columns_a.clone(), tie.columns_b.clone()))
        .collect();
    ties.sort_unstable();
    ties.dedup();
    found.extend(ties);
    // A degenerate cross can have a great many equally scoring assignments.
    // The best one stays; the rest are compared up to this many.
    found.truncate(CANDIDATE_CAP);
    found
}

/// The canonical reconstructions worth comparing for one column assignment.
///
/// Canonical form is fixed by the rows and the paths together, and the paths
/// are chosen by the same order, so every relabelling that leaves the rows
/// ascending gets its own smallest paths.
fn relabellings(
    tables: &Tables,
    columns_a: &[usize],
    columns_b: &[usize],
    candidates: usize,
) -> Vec<Reconstruction> {
    let haps_a = haplotypes_of(tables, Parent::A, columns_a);
    let haps_b = haplotypes_of(tables, Parent::B, columns_b);
    let perms_a = tiebreak::sorting_permutations(&haps_a);
    let perms_b = tiebreak::sorting_permutations(&haps_b);
    let wide = candidates * perms_a.len() * perms_b.len() <= FRAME_BUDGET;

    let mut out = Vec::new();
    for perm_a in &perms_a {
        for perm_b in &perms_b {
            out.push(assemble(tables, columns_a, columns_b, perm_a, perm_b));
            if !wide {
                return out;
            }
        }
    }
    out
}

/// Builds a reconstruction from a column choice per parent and marker.
///
/// The paths are the smallest optimal ones in the relabelling given.
fn assemble(
    tables: &Tables,
    columns_a: &[usize],
    columns_b: &[usize],
    perm_a: &[u8; HOMOLOGS],
    perm_b: &[u8; HOMOLOGS],
) -> Reconstruction {
    let mut answer = Reconstruction {
        haplotypes_a: haplotypes_of(tables, Parent::A, columns_a),
        haplotypes_b: haplotypes_of(tables, Parent::B, columns_b),
        paths: Vec::with_capacity(tables.offspring),
    };
    let paths = tiebreak::minimal_paths(tables, columns_a, columns_b, perm_a, perm_b).1;
    for joint in &paths {
        let mut from_a = Vec::with_capacity(tables.markers);
        let mut from_b = Vec::with_capacity(tables.markers);
        for &j in joint {
            from_a.push(ALL_STATES[j / core::NS]);
            from_b.push(ALL_STATES[j % core::NS]);
        }
        answer.paths.push(ProgenyPath { from_a, from_b });
    }
    canonicalize(&mut answer);
    answer
}

/// Expands per marker column masks into four homolog rows.
fn haplotypes_of(tables: &Tables, parent: Parent, columns: &[usize]) -> Haplotypes {
    let index = match parent {
        Parent::A => core::PA,
        Parent::B => core::PB,
    };
    let mut haps = Haplotypes::zeros(tables.markers);
    for (m, &option) in columns.iter().enumerate() {
        let mask = tables.cols[index][m][option];
        for h in 0..HOMOLOGS {
            haps.set(h, m, (mask >> h) & 1);
        }
    }
    haps
}

/// All permutations of the four homolog indices, in ascending image order.
///
/// Canonical form is defined over this set. Which permutation it picks is
/// decided by [`canonical::canonicalize`], not here.
pub fn homolog_permutations() -> Vec<[u8; HOMOLOGS]> {
    canonical::homolog_permutations()
}
