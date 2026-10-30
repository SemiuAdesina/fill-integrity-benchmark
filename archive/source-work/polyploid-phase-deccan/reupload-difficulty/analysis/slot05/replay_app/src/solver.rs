
//! The search.
//!
//! The objective is a function of the parental haplotype assignment alone: fix
//! it and the progeny are independent, each a Viterbi problem over the hundred
//! joint inheritance states. So the search is over assignments, and it is made
//! exact rather than heuristic by three things.
//!
//! * **A bound.** Every term is a log likelihood, so no term is positive. A
//!   partially fixed assignment carries, per progeny, the best score of the
//!   markers settled so far; a precomputed table carries the best continuation
//!   if every later marker were free to take its most favourable column. Their
//!   sum bounds any completion of that prefix, so whole subtrees are dropped
//!   once they cannot beat the best assignment already found.
//! * **Symmetry.** Relabelling a parent's homologs does not change the score,
//!   so only one assignment per orbit is visited: at each marker the search
//!   offers just the columns that are least in their orbit under the
//!   relabellings still fixing everything chosen so far.
//! * **A lower bound first.** Alternating optimisation — best paths given the
//!   assignment, best columns given the paths — is run from many random starts.
//!   It is not trusted to be optimal; it is what the bound prunes against, and
//!   a tight one is what keeps the tree small.
//!
//! What is left after the optimum is known is which optimal reconstruction to
//! report, since several can tie. `canon` answers that directly from
//! `spec/result-format-v1.md`.

use crate::cross_input::Cross;
use crate::cross_model::{Parent, Reconstruction, HOMOLOGS};

pub mod branch;
pub mod canon;
pub mod dual;
pub mod model;

use branch::{branch_and_bound, local_search, polish};
use canon::{build_paths, canonicalise, distinct_key, haplotypes_a, haplotypes_b, sorting_permutations};
use model::Model;

/// Random starts for the alternating optimisation.
const RESTARTS: u64 = 96;
/// Subgradient steps spent tightening the bound.
const DUAL_STEPS: usize = 200;

fn dual_steps() -> usize {
    std::env::var("PHASE_DUAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DUAL_STEPS)
}
/// Rounds of alternating optimisation spent on a candidate from the bound.
const POLISH_ROUNDS: usize = 12;

/// Finds the highest scoring reconstruction for a cross.
///
/// The returned reconstruction is structurally valid, globally optimal, and in
/// canonical form: the search proves the optimum, and every reconstruction that
/// ties with it is built and the least one under the key the result format
/// fixes is the one returned.
pub fn solve(cross: &Cross) -> Reconstruction {
    let model = Model::new(cross);
    let t0 = std::time::Instant::now();
    let (lower_bound, fallback) = local_search(&model, RESTARTS);
    let tightened = dual::optimise(&model, lower_bound, dual_steps());
    // The multipliers name an assignment per progeny and one they agree on; each
    // is feasible, so each is worth improving into a candidate optimum.
    let mut incumbent = (lower_bound, fallback);
    if let Ok(hint) = std::env::var("PHASE_LB") {
        if let Ok(v) = hint.parse::<f64>() {
            incumbent.0 = v;
        }
    }
    for candidate in &tightened.candidates {
        let polished = polish(&model, candidate, POLISH_ROUNDS);
        if polished.0 > incumbent.0 {
            incumbent = polished;
        }
    }
    let t1 = std::time::Instant::now();
    let outcome = branch_and_bound(&model, incumbent.0, &tightened.dual);
    let t2 = std::time::Instant::now();
    if std::env::var("PHASE_TIMING").is_ok() {
        eprintln!(
            "[timing] local={:.3}s bnb={:.3}s nodes={} leaves={} ties={} best={:.6} lb={:.6} root={:.6}",
            (t1 - t0).as_secs_f64(),
            (t2 - t1).as_secs_f64(),
            outcome.nodes,
            outcome.leaves,
            outcome.assignments.len(),
            outcome.best,
            incumbent.0,
            outcome.root_bound,
        );
    }

    let mut assignments = outcome.assignments;
    if assignments.is_empty() {
        assignments.push(incumbent.1);
    }

    let mut best: Option<(Vec<u8>, Reconstruction)> = None;
    for assign in &assignments {
        let haplotypes_a = haplotypes_a(&model, assign);
        let haplotypes_b = haplotypes_b(&model, assign);
        let skeleton = Reconstruction {
            haplotypes_a: haplotypes_a.clone(),
            haplotypes_b: haplotypes_b.clone(),
            paths: Vec::new(),
        };
        // Relabellings that sort the rows can differ by how they rewrite the
        // paths, and only the paths can tell them apart. Trying every pair is
        // what makes the tie break agree with the canonical form rather than
        // with one labelling of it.
        for perm_a in sorting_permutations(&skeleton, Parent::A) {
            for perm_b in sorting_permutations(&skeleton, Parent::B) {
                let paths = build_paths(&model, assign, &perm_a, &perm_b);
                let answer = canonicalise(&Reconstruction {
                    haplotypes_a: haplotypes_a.clone(),
                    haplotypes_b: haplotypes_b.clone(),
                    paths,
                });
                let key = distinct_key(&answer);
                if best.as_ref().is_none_or(|(held, _)| key < *held) {
                    best = Some((key, answer));
                }
            }
        }
    }

    best.expect("the search always reaches at least one assignment").1
}

/// All permutations of the four homolog indices, in ascending image order.
///
/// Canonical form is defined over this set. Which permutation to choose is the
/// part that matters, and that is decided in [`canon`].
pub fn homolog_permutations() -> Vec<[u8; HOMOLOGS]> {
    canon::perms4()
}
