
//! The fixed list of candidate executions a graded run performs.
//!
//! This module exists so that "how often is the candidate's solver run" is a
//! property of the verifier's own source rather than an emergent consequence of
//! how many assertions happen to want a reconstruction. Each entry below is one
//! process, one call into the candidate's `solve`, and one artifact. Every
//! semantic group then reads artifacts; none of them can cause another
//! execution.
//!
//! A job is identified by a `(stem, transform)` pair. Two jobs share a stem only
//! when the assertion is itself about repeated execution — group K asks whether
//! solving the same cross twice gives the same answer, and that question cannot
//! be answered from one execution. Nothing here re-solves a cross merely because
//! several assertions inspect its answer.

use crate::cross_input::Cross;

/// The hidden corpus, in the order the structural groups sweep it.
pub const ALL_FIXTURES: [&str; 9] = [
    "baseline", "greedy_trap", "dr_legality", "symmetry", "recombination", "ties", "compound",
    "coupled_ties", "path_order",
];

/// Constant added to every genotype likelihood by the shifted jobs.
///
/// Group M relies on this being uniform across progeny, markers and dosages:
/// every reconstruction's total moves by the same amount, so the maximiser, the
/// canonical form and the tie ordering are all unchanged.
pub const LIKELIHOOD_SHIFT: f64 = -0.25;

/// What the runner does to a cross before handing it to the candidate.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Transform {
    /// The cross exactly as the fixture ships it.
    AsSupplied,
    /// Every genotype likelihood moved by [`LIKELIHOOD_SHIFT`].
    ShiftLikelihoods,
}

impl Transform {
    pub fn apply(self, cross: &mut Cross) {
        match self {
            Transform::AsSupplied => {}
            Transform::ShiftLikelihoods => {
                for progeny in cross.child_gl.iter_mut() {
                    for marker in progeny.iter_mut() {
                        for value in marker.iter_mut() {
                            *value += LIKELIHOOD_SHIFT;
                        }
                    }
                }
            }
        }
    }
}

/// One candidate execution: which fixture, in what form, under what name.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ProbeJob {
    /// Artifact name. Unique across the plan.
    pub id: &'static str,
    /// Fixture whose cross is solved.
    pub stem: &'static str,
    pub transform: Transform,
}

/// Registers one job. One invocation per line, which is how the runner counts
/// the executions the plan defines without trusting anything it built.
macro_rules! job {
    ($id:literal, $stem:literal, $transform:ident) => {
        ProbeJob { id: $id, stem: $stem, transform: Transform::$transform }
    };
}

/// Every candidate execution a graded run performs, in execution order.
///
/// Nine crosses as supplied, three solved a second time for group K, three
/// shifted for group M. Fifteen processes, fifteen artifacts.
pub const PLAN: [ProbeJob; 15] = [
    job!("baseline", "baseline", AsSupplied),
    job!("greedy_trap", "greedy_trap", AsSupplied),
    job!("dr_legality", "dr_legality", AsSupplied),
    job!("symmetry", "symmetry", AsSupplied),
    job!("recombination", "recombination", AsSupplied),
    job!("ties", "ties", AsSupplied),
    job!("compound", "compound", AsSupplied),
    job!("coupled_ties", "coupled_ties", AsSupplied),
    job!("path_order", "path_order", AsSupplied),
    job!("ties_again", "ties", AsSupplied),
    job!("symmetry_again", "symmetry", AsSupplied),
    job!("compound_again", "compound", AsSupplied),
    job!("baseline_shifted", "baseline", ShiftLikelihoods),
    job!("symmetry_shifted", "symmetry", ShiftLikelihoods),
    job!("compound_shifted", "compound", ShiftLikelihoods),
];

/// The job with this identifier.
pub fn find(id: &str) -> Option<&'static ProbeJob> {
    PLAN.iter().find(|job| job.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_identifiers_are_unique() {
        for (i, job) in PLAN.iter().enumerate() {
            assert!(
                PLAN[..i].iter().all(|earlier| earlier.id != job.id),
                "duplicate job identifier {}",
                job.id
            );
        }
    }

    #[test]
    fn every_fixture_is_solved_as_supplied_exactly_once() {
        for stem in ALL_FIXTURES {
            let n = PLAN
                .iter()
                .filter(|job| job.stem == stem && job.transform == Transform::AsSupplied)
                .count();
            // Three fixtures are solved a second time for group K, which is an
            // assertion about repeated execution rather than a second reader of
            // one answer.
            let expected = if ["ties", "symmetry", "compound"].contains(&stem) { 2 } else { 1 };
            assert_eq!(n, expected, "{stem} is solved {n} times as supplied");
        }
    }

    #[test]
    fn every_job_names_a_known_fixture() {
        for job in PLAN {
            assert!(ALL_FIXTURES.contains(&job.stem), "{} names no known fixture", job.stem);
            assert_eq!(find(job.id), Some(&job));
        }
    }
}
