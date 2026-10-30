
//! The search.
//!
//! The objective is a sum over progeny of a maximum over inheritance paths, so
//! the search works on the two things that decide it: the parents' allele
//! columns, which every progeny sees, and each progeny's path, which only sees
//! those columns. Fix the columns and every progeny is a small Viterbi problem;
//! fix the paths and every marker's columns can be chosen on its own.
//!
//! The columns are what is searched over, and there are three searches, tried in
//! order of what they cost and what they return:
//!
//! 1. enumeration, where the admissible space is small enough to walk;
//! 2. a beam over the marker chain (`solver/beam.rs`), which ends on a handful
//!    of good complete assignments for the price of one forward pass per child;
//! 3. branch and bound (`solver/exact.rs`), which the beam's assignments give an
//!    incumbent to prune against. Where it closes, the answer is proven optimal
//!    and every assignment attaining the optimum is in hand; where it does not,
//!    the cross is too long and the search falls back to improving what it has.
//!
//! Improvement is `solver/search.rs`: the two conditional moves above, run to a
//! fixed point, then perturbed and re-improved, which is what climbs out of the
//! local optima the beam lands in.
//!
//! Reaching the best score is only half of the job. Several reconstructions can
//! reach it, so the answer is reduced to the canonical form
//! `spec/result-format-v1.md` requires and the smallest among equals is
//! reported, which is where `solver/tiebreak.rs` lives.

mod canon;
mod search;
mod tables;
mod tiebreak;

use crate::cross_input::Cross;
use crate::cross_model::{Reconstruction, HOMOLOGS};
use crate::scoring::{score, SCORE_TOLERANCE};
use crate::solver::canon::canonicalise;
use crate::solver::tables::Ctx;
use crate::solver::tiebreak::{candidates, reconstruction};

/// How many equally scoring column assignments are examined for the smallest
/// canonical reconstruction. Ties are common; examining all of them is not
/// affordable, and ties beyond this many rarely change the representative.
const ASSIGNMENTS_EXAMINED: usize = 64;

/// Finds the highest scoring reconstruction for a cross.
///
/// The returned reconstruction is structurally valid, globally optimal, and in
/// canonical form.
pub fn solve(cross: &Cross) -> Reconstruction {
    let ctx = Ctx::new(cross);
    let outcome = close(&ctx, search::run(&ctx));

    let mut pool: Vec<(f64, Vec<u8>, Reconstruction)> = Vec::new();
    for (ca, cb) in outcome.assignments.iter().take(ASSIGNMENTS_EXAMINED) {
        for paths in candidates(&ctx, ca, cb) {
            let mut answer = reconstruction(&ctx, ca, cb, &paths);
            canonicalise(&mut answer);
            // Only reconstructions that really do attain the optimum compete
            // for the representative: a smaller key beside a smaller score is
            // not a better answer.
            let value = match score(cross, &answer) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let key = canon::distinct_key(&answer);
            pool.push((value, key, answer));
        }
    }

    let mut best_value = f64::NEG_INFINITY;
    for (value, _, _) in &pool {
        if *value > best_value {
            best_value = *value;
        }
    }
    pool.retain(|(value, _, _)| (*value - best_value).abs() <= SCORE_TOLERANCE);

    let mut best: Option<(Vec<u8>, Reconstruction)> = None;
    for (_, key, answer) in pool {
        if best.as_ref().is_none_or(|(held, _)| key < *held) {
            best = Some((key, answer));
        }
    }

    match best {
        Some((_, answer)) => answer,
        None => fallback(&ctx),
    }
}

/// Closes the gap between what the search reached and the optimum.
///
/// Below [`EXACT_MARKERS`] markers the whole tree can be searched, so the answer
/// is not merely the best one found but the best there is. Above that the search
/// is run anyway against a work limit, since it often improves on the incumbent
/// before it runs out; it is never allowed to make the answer worse.
fn close(ctx: &Ctx, mut outcome: search::Outcome) -> search::Outcome {
    if outcome.enumerated || ctx.m == 0 || ctx.m > EXACT_MARKERS {
        return outcome;
    }
    let limit = ((EXACT_WORK / (ctx.n as f64 * 2100.0)) as u64).clamp(1_000, 2_000_000);
    let mut exact = Exact::new(ctx, limit);
    exact.seed(outcome.value);
    if exact.proves(outcome.value) {
        return outcome;
    }
    exact.run();
    if exact.best() <= outcome.value + 1e-9 {
        return outcome;
    }
    let mut ties = exact.ties();
    if ties.is_empty() {
        let (ca, cb) = exact.columns();
        ties.push((ca, cb));
    }
    search::Outcome { value: exact.best(), assignments: ties, enumerated: false }
}

/// Used only if the search produced nothing, which cannot happen for a valid
/// cross: every marker admits at least one column, so there is always a
/// reconstruction to report.
fn fallback(ctx: &Ctx) -> Reconstruction {
    let ca = vec![0u8; ctx.m];
    let cb = vec![0u8; ctx.m];
    let paths: Vec<Vec<usize>> = (0..ctx.n).map(|c| ctx.viterbi(c, &ca, &cb).path()).collect();
    let mut answer = reconstruction(ctx, &ca, &cb, &paths);
    canonicalise(&mut answer);
    answer
}

/// All permutations of the four homolog indices, in ascending image order.
pub fn homolog_permutations() -> Vec<[u8; HOMOLOGS]> {
    canon::permutations()
}
