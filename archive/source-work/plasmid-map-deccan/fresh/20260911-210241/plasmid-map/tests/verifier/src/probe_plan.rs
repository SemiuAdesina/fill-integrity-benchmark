
//! The fixed list of candidate executions a graded run performs.
//!
//! Each entry is one process, one call into the candidate's `solve`, and one
//! artifact. Every check reads artifacts; none can cause another execution,
//! so how often the candidate runs is a property of this file, which the
//! runner counts from the source text.

use crate::input::Problem;

/// The hidden corpus.
///
/// - `public`: the problem in `/app`, solved twice for the agreement check.
/// - `h_parsimony`: the least map has fewer insert-region sites than the
///   construct the gels came from; a search that fits first and counts
///   second returns the wrong map.
/// - `h_sub_detection`: a single digest is missing a fragment below the
///   detection limit, so band counts understate cut counts and the sum of
///   bands understates the construct.
/// - `h_comigration`: an observed band carries two fragments.
/// - `h_junction`: a backbone window torn by the insertion, or a cut at a
///   junction, so the required and insert-region sets are exactly the
///   specification's and not a naive scan of the backbone.
/// - `h_ties`: an enzyme whose two insert-region cuts no digest orders; the
///   two orderings tie on count and deviation and the position order decides.
/// - `h_cloning_sites`: four listed cloning sites and the answer is not at
///   the first.
/// - `h_three_sites`: an enzyme with three insert-region sites.
/// - `s_large_a`, `s_large_b`, `s_large_c`: five enzymes and seven or more
///   insert-region sites, where enumerating tolerance-consistent
///   configurations per enzyme does not finish.
/// - `h_low_error`: one percent sizing error.
pub const ALL_FIXTURES: [&str; 12] = [
    "public", "h_parsimony", "h_sub_detection", "h_comigration", "h_junction", "h_ties", "h_cloning_sites", "h_three_sites",
    "s_large_a", "s_large_b", "s_large_c", "h_low_error",
];

/// What the runner does to a problem before handing it to the candidate.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Transform {
    /// The problem exactly as the fixture ships it.
    AsSupplied,
    /// The backbone rotated so that it starts `rotation` bases later, with
    /// the cloning sites moved along; the gels are unchanged. The construct is
    /// the same molecule read from another origin, so the answer is the
    /// certified map rotated the same way.
    Rotate(usize),
}

impl Transform {
    pub fn apply(self, problem: &mut Problem) {
        match self {
            Transform::AsSupplied => {}
            Transform::Rotate(r) => {
                let v = problem.v();
                let r = r % v;
                let mut seq = problem.backbone.seq[r..].to_vec();
                seq.extend_from_slice(&problem.backbone.seq[..r]);
                problem.backbone.seq = seq;
                for site in problem.cloning_sites.iter_mut() {
                    site.position = (site.position + v - r) % v;
                }
                problem.cloning_sites.sort_by_key(|s| s.position);
            }
        }
    }
}

/// One candidate execution.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ProbeJob {
    pub id: &'static str,
    pub stem: &'static str,
    pub transform: Transform,
}

macro_rules! job {
    ($id:literal, $stem:literal, $transform:expr) => {
        ProbeJob { id: $id, stem: $stem, transform: $transform }
    };
}

/// Every candidate execution a graded run performs, in execution order:
/// twelve problems as supplied, three solved again, three rotated.
pub const PLAN: [ProbeJob; 18] = [
    job!("public", "public", Transform::AsSupplied),
    job!("h_parsimony", "h_parsimony", Transform::AsSupplied),
    job!("h_sub_detection", "h_sub_detection", Transform::AsSupplied),
    job!("h_comigration", "h_comigration", Transform::AsSupplied),
    job!("h_junction", "h_junction", Transform::AsSupplied),
    job!("h_ties", "h_ties", Transform::AsSupplied),
    job!("h_cloning_sites", "h_cloning_sites", Transform::AsSupplied),
    job!("h_three_sites", "h_three_sites", Transform::AsSupplied),
    job!("h_low_error", "h_low_error", Transform::AsSupplied),
    job!("s_large_a", "s_large_a", Transform::AsSupplied),
    job!("s_large_b", "s_large_b", Transform::AsSupplied),
    job!("s_large_c", "s_large_c", Transform::AsSupplied),
    job!("public_again", "public", Transform::AsSupplied),
    job!("h_ties_again", "h_ties", Transform::AsSupplied),
    job!("s_large_a_again", "s_large_a", Transform::AsSupplied),
    job!("public_rotated", "public", Transform::Rotate(1237)),
    job!("h_low_error_rotated", "h_low_error", Transform::Rotate(611)),
    job!("h_three_sites_rotated", "h_three_sites", Transform::Rotate(2003)),
];

/// The job with this identifier.
pub fn find(id: &str) -> Option<&'static ProbeJob> {
    PLAN.iter().find(|job| job.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_identifiers_are_unique_and_name_known_fixtures() {
        for (i, job) in PLAN.iter().enumerate() {
            assert!(PLAN[..i].iter().all(|earlier| earlier.id != job.id), "duplicate job {}", job.id);
            assert!(ALL_FIXTURES.contains(&job.stem));
            assert_eq!(find(job.id), Some(job));
        }
        for stem in ALL_FIXTURES {
            assert!(PLAN.iter().any(|j| j.stem == stem && j.transform == Transform::AsSupplied));
        }
    }
}
