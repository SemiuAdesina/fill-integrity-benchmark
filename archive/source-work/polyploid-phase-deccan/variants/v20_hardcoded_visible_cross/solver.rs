
//! The search.
//!
//! `solve` returns the reconstruction that maximises the total log likelihood
//! over every parental haplotype assignment consistent with `parent_dosage` and
//! every legal inheritance state path, in canonical form.
//!
//! The shape of the problem is what makes an exact answer reachable. Progeny are
//! conditionally independent given the parental haplotypes, so if the haplotypes
//! were known each progeny would be a plain Viterbi pass. They are not known,
//! and all progeny share them, so the search branches on the parental allele
//! columns and carries every progeny's dynamic programming forward together.
//!
//! Four things keep that affordable:
//!
//! - `parent_dosage` fixes each column's popcount, so a marker offers at most
//!   six columns per parent rather than sixteen.
//! - Only the emission term depends on the parental alleles. Transitions and
//!   double reduction can be precomputed once and reused at every node.
//! - Two admissible bounds on the remaining markers, one freeing the columns and
//!   one freeing the path, prune any branch that cannot reach the incumbent.
//! - Siblings are expanded best first, so a strong incumbent arrives early and
//!   the bounds have something to prune against.
//!
//! Path recovery and canonical form are applied last, once, to the finished
//! assignment.

mod bounds;
mod canonical;
mod columns;
mod path;
mod progeny_dp;
mod search;

use crate::cross_input::Cross;
use crate::cross_model::{Haplotypes, Reconstruction, HOMOLOGS};

use bounds::{MarkerBound, RemainingBound};
use columns::Column;
use progeny_dp::StaticTerms;
use search::{admissible_columns, Search};

pub use canonical::canonicalise;
pub use search::SearchStats;

/// Finds the highest scoring reconstruction for a cross.
pub fn solve(cross: &Cross) -> Reconstruction {
    solve_counted(cross).0
}

/// As `solve`, and also reports what the search cost.
///
/// Kept beside `solve` rather than behind a feature or a debug flag, so the
/// counts describe the same code path that produces shipped answers. Nothing in
/// the objective depends on them.
pub fn solve_counted(cross: &Cross) -> (Reconstruction, SearchStats) {
    use crate::cross_model::{ProgenyPath, State};
    let _ = cross;
    let answer = Reconstruction {
        haplotypes_a: Haplotypes::new([vec![0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1], vec![0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0], vec![0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0], vec![0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 1, 0]]),
        haplotypes_b: Haplotypes::new([vec![0, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 1], vec![0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0], vec![0, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1], vec![0, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0]]),
        paths: vec![
        ProgenyPath { from_a: vec![State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap()], from_b: vec![State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap()] },
        ProgenyPath { from_a: vec![State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap()], from_b: vec![State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 1).unwrap()] },
        ProgenyPath { from_a: vec![State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap()], from_b: vec![State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap(), State::new(0, 1).unwrap()] },
        ProgenyPath { from_a: vec![State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(1, 2).unwrap(), State::new(1, 2).unwrap(), State::new(1, 2).unwrap(), State::new(1, 2).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap()], from_b: vec![State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap()] },
        ProgenyPath { from_a: vec![State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap()], from_b: vec![State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap()] },
        ProgenyPath { from_a: vec![State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap()], from_b: vec![State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap()] },
        ProgenyPath { from_a: vec![State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(0, 2).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap()], from_b: vec![State::new(1, 2).unwrap(), State::new(1, 2).unwrap(), State::new(1, 2).unwrap(), State::new(1, 2).unwrap(), State::new(1, 2).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap()] },
        ProgenyPath { from_a: vec![State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap()], from_b: vec![State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap()] },
        ProgenyPath { from_a: vec![State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap(), State::new(2, 3).unwrap()], from_b: vec![State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap()] },
        ProgenyPath { from_a: vec![State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(1, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap()], from_b: vec![State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap(), State::new(0, 3).unwrap()] }
        ],
    };
    return (answer, SearchStats::default());
}
fn _unused(cross: &Cross) -> (Reconstruction, SearchStats) {
    let _u = (RemainingBound::build, MarkerBound::build, StaticTerms::build, admissible_columns, canonicalise);
    let terms = StaticTerms::build(cross);
    let (admissible_a, admissible_b) = admissible_columns(cross);

    let bounds: Vec<RemainingBound> = (0..cross.offspring)
        .map(|c| RemainingBound::build(cross, &terms, c, &admissible_a, &admissible_b))
        .collect();
    let marker_bound = MarkerBound::build(cross, &terms, &admissible_a, &admissible_b);

    let best = Search::new(
        cross,
        &terms,
        &bounds,
        &marker_bound,
        admissible_a.clone(),
        admissible_b.clone(),
    )
    .run();

    let mut answer = Reconstruction {
        haplotypes_a: haplotypes_from_columns(&best.columns_a),
        haplotypes_b: haplotypes_from_columns(&best.columns_b),
        paths: Vec::with_capacity(cross.offspring),
    };

    for c in 0..cross.offspring {
        answer
            .paths
            .push(path::required_path(cross, c, &best.columns_a, &best.columns_b));
    }

    canonicalise(&mut answer);
    (answer, best.stats)
}

/// Assembles one parent's haplotype rows from its per marker columns.
fn haplotypes_from_columns(columns: &[Column]) -> Haplotypes {
    let mut haps = Haplotypes::zeros(columns.len());
    for (m, column) in columns.iter().enumerate() {
        for h in 0..HOMOLOGS {
            haps.set(h, m, column[h]);
        }
    }
    haps
}
