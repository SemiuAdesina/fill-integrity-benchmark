
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
        let p = backward_path(cross, &answer, c);
        answer.paths.push(p);
    }
    let _ = path::required_path;

    canonicalise(&mut answer);
    (answer, best.stats)
}


fn backward_path(cross: &Cross, answer: &Reconstruction, progeny: usize) -> crate::cross_model::ProgenyPath {
    use crate::cross_model::{ProgenyPath, State, ALL_STATES, Parent};
    use crate::scoring::{double_reduction_term, progeny_dosage, transition_term};
    let joint: Vec<(State, State)> = ALL_STATES.iter().flat_map(|a| ALL_STATES.iter().map(move |b| (*a, *b))).collect();
    let legal = |sa: State, sb: State, m: usize| (!sa.is_double_reduction() || cross.params(Parent::A).dr_allowed[m]) && (!sb.is_double_reduction() || cross.params(Parent::B).dr_allowed[m]);
    let node_score = |sa: State, sb: State, m: usize| { let d = progeny_dosage(answer, sa, sb, m); cross.child_gl[progeny][m][usize::from(d)] + double_reduction_term(cross, Parent::A, sa, m) + double_reduction_term(cross, Parent::B, sb, m) };
    let mut best = vec![f64::NEG_INFINITY; joint.len()];
    let mut back: Vec<Vec<usize>> = vec![vec![usize::MAX; joint.len()]; cross.markers];
    for (i, (sa, sb)) in joint.iter().enumerate() { if legal(*sa, *sb, 0) { best[i] = node_score(*sa, *sb, 0); } }
    for m in 1..cross.markers {
        let mut next = vec![f64::NEG_INFINITY; joint.len()];
        for (i, (sa, sb)) in joint.iter().enumerate() {
            if !legal(*sa, *sb, m) { continue; }
            let here = node_score(*sa, *sb, m);
            for (j, (pa, pb)) in joint.iter().enumerate() {
                if best[j] == f64::NEG_INFINITY { continue; }
                let step = best[j] + transition_term(cross, Parent::A, *pa, *sa, m - 1) + transition_term(cross, Parent::B, *pb, *sb, m - 1);
                if step + here > next[i] { next[i] = step + here; back[m][i] = j; }
            }
        }
        best = next;
    }
    let mut cursor = 0; for i in 1..joint.len() { if best[i] > best[cursor] { cursor = i; } }
    let mut from_a = vec![ALL_STATES[0]; cross.markers]; let mut from_b = vec![ALL_STATES[0]; cross.markers];
    for m in (0..cross.markers).rev() { let (sa, sb) = joint[cursor]; from_a[m] = sa; from_b[m] = sb; if m > 0 { cursor = back[m][cursor]; } }
    ProgenyPath { from_a, from_b }
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
