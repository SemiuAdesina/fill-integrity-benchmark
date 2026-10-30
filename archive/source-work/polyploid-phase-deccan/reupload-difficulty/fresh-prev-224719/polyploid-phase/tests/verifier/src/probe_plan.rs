
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
///
/// The last two crosses carry more read evidence than the rest and are the only
/// ones whose optimum takes the reference oracle appreciable time to prove.
/// They are here because the first nine are all small enough that a search
/// which relaxes the shared parental columns one progeny at a time still closes
/// them quickly, so the corpus said nothing about whether a solver's optimism
/// about the undecided markers is strong enough to survive a cross where that
/// relaxation is loose.
///
/// - `shared_phase` is a duplex-rich, tightly linked cross: eleven of its
///   twenty-four parental marker dosages are two, so six allele columns remain
///   admissible at each of them, and the median recombination fraction is
///   below a tenth, so a phase choice made at one marker is still felt many
///   markers away. Its optimum contains twelve recombination events and is
///   attained by two distinct parental assignments that tie within `1e-8`, so
///   a search that discards equal-scoring branches, or that stops at the first
///   optimum it reaches, reports the wrong one of the two.
/// - `low_information` is the low-depth case: sixty per cent of its
///   marker-progeny cells separate their best dosage from the next by less
///   than half a nat, so most progeny leave several parental columns plausible
///   and the optimum is decided by what all ten progeny agree on rather than
///   by any one of them.
/// - `unlinked_interval` carries two intervals per parent at a recombination
///   fraction of exactly one half, which the input format permits and which a
///   marker pair far enough apart on a sparse map really has. At `r = 0.5` the
///   transition term is `ln(0.5)` whichever states the interval joins, so
///   switching costs nothing and the co-optimal set explodes: fifty-five
///   thousand two hundred and ninety-six distinct parental assignments attain
///   this cross's optimum, and exactly one of them is the answer the result
///   format names. Every other fixture in the corpus has at most two. A search
///   that discards a branch which can only tie, that keeps the first optimum it
///   reaches, or that recovers a path by preferring the state it is already in,
///   returns one of the other fifty-five thousand.
///
/// Both were kept only because the exact reference oracle proved their optimum
/// inside a fixed budget; neither is enumerable, and neither is retained for
/// its running time.
///
/// The last three are small crosses at the minimum size, with most parental
/// dosages pinned to 0 or 4 so that every admissible parental assignment can be
/// enumerated outright. Each grades a rule the specification states and the
/// larger crosses happen not to reach.
///
/// - `marker0_dr`: at the first marker parent A is simplex with double
///   reduction admissible and parent B is nulliplex with it forbidden, so two
///   alternate copies in a progeny there can come only from A receiving the
///   same homolog twice; two progeny do. At the second marker the roles are
///   reversed and the form is forbidden in both parents, and a progeny's reads
///   there favour a dosage that only the forbidden form could give. The legal
///   optimum settles for the lower dosage. A solver that forbids the form
///   everywhere misses the first; one that penalises it instead of forbidding
///   it takes the second; one that decides legality per parent but not per
///   marker, or that skips the first marker because no transition precedes
///   it, fails one or the other.
/// - `low_depth`: eight progeny-marker cells on segregating markers carry no
///   read evidence at all — five equal likelihoods — and six more separate the
///   dosages by hundredths of a nat. The optimum there is decided by linkage to
///   the neighbouring markers and by the parental phase the other progeny fix,
///   and the certified answer contains three recombination events. Its optimum
///   is unique up to relabelling: what the uninformative cells test is the
///   inference across markers, not a tie rule.
/// - `unlinked_ties`: four intervals at `r = 0.5`, the unlinked value the
///   format permits, at which a switch costs exactly as much as staying put;
///   both parents carry three identical homolog rows; and three distinct
///   canonical reconstructions attain the optimum within `1e-8`. Equality
///   retention, the row-signature rule and the distinct-optimum order all have
///   to be right at once. It replaces `ties`, whose optimum was unique.
pub const ALL_FIXTURES: [&str; 13] = [
    "baseline", "greedy_trap", "dr_legality", "symmetry", "compound", "coupled_ties", "path_order",
    "shared_phase", "low_information", "unlinked_interval", "marker0_dr", "low_depth",
    "unlinked_ties",
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
/// Thirteen crosses as supplied, three solved a second time for group K, three
/// shifted for group M. Nineteen processes, nineteen artifacts.
pub const PLAN: [ProbeJob; 19] = [
    job!("baseline", "baseline", AsSupplied),
    job!("greedy_trap", "greedy_trap", AsSupplied),
    job!("dr_legality", "dr_legality", AsSupplied),
    job!("symmetry", "symmetry", AsSupplied),
    job!("compound", "compound", AsSupplied),
    job!("coupled_ties", "coupled_ties", AsSupplied),
    job!("path_order", "path_order", AsSupplied),
    job!("shared_phase", "shared_phase", AsSupplied),
    job!("low_information", "low_information", AsSupplied),
    job!("unlinked_interval", "unlinked_interval", AsSupplied),
    job!("marker0_dr", "marker0_dr", AsSupplied),
    job!("low_depth", "low_depth", AsSupplied),
    job!("unlinked_ties", "unlinked_ties", AsSupplied),
    job!("unlinked_ties_again", "unlinked_ties", AsSupplied),
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
            let expected = if ["unlinked_ties", "symmetry", "compound"].contains(&stem) { 2 } else { 1 };
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
