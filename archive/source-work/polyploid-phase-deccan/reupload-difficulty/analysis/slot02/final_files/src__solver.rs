
//! The search.
//!
//! `solve` returns the reconstruction that maximises the total log likelihood
//! over every parental haplotype assignment consistent with `parent_dosage` and
//! every legal inheritance state path, in the canonical form
//! `spec/result-format-v1.md` describes.
//!
//! The work is split over the four helper modules:
//!
//! - [`tables`] flattens the cross into the arrays the inner loops need.
//! - [`dp`] solves one progeny's path against fixed parental columns, and
//!   recovers the smallest optimal path when several tie.
//! - [`canon`] implements canonical form and the order between distinct optima.
//! - [`search`] drives the three exact steps, paths against columns, columns
//!   against paths and one marker against the paths around it, from many starts
//!   in a population search, and enumerates outright when the admissible space
//!   is small enough to list.

pub mod canon;
pub mod dp;
pub mod search;
pub mod tables;

use crate::cross_input::Cross;
use crate::cross_model::{Haplotypes, ProgenyPath, Reconstruction, State, HOMOLOGS};

use search::Solution;
use tables::{SA, SB};

/// Finds the highest scoring reconstruction for a cross.
///
/// The returned reconstruction is structurally valid, attains the global optimum
/// over parental assignments and inheritance paths jointly, and is in canonical
/// form.
pub fn solve(cross: &Cross) -> Reconstruction {
    let ctx = tables::Ctx::new(cross);
    let solution = search::run(&ctx);
    to_reconstruction(&ctx, &solution)
}

/// Turns the solver's own representation back into the crate's domain types.
fn to_reconstruction(ctx: &tables::Ctx, solution: &Solution) -> Reconstruction {
    let haplotypes_a = haplotypes(&solution.cols_a, ctx.markers);
    let haplotypes_b = haplotypes(&solution.cols_b, ctx.markers);

    let mut paths = Vec::with_capacity(ctx.offspring);
    for c in 0..ctx.offspring {
        let from_a = solution.pa[c]
            .iter()
            .map(|s| state(*s))
            .collect::<Vec<State>>();
        let from_b = solution.pb[c].iter().map(|s| state(*s)).collect::<Vec<State>>();
        paths.push(ProgenyPath { from_a, from_b });
    }

    Reconstruction { haplotypes_a, haplotypes_b, paths }
}

fn haplotypes(cols: &[u8], markers: usize) -> Haplotypes {
    let mut haps = Haplotypes::zeros(markers);
    for h in 0..HOMOLOGS {
        for m in 0..markers {
            haps.set(h, m, (cols[m] >> h) & 1);
        }
    }
    haps
}

fn state(index: u8) -> State {
    State::new(SA[usize::from(index)], SB[usize::from(index)])
        .expect("the solver only produces state indices below ten")
}

/// All permutations of the four homolog indices, in ascending image order.
///
/// Canonical form is defined over this set. Which permutation to choose is
/// decided in [`canon`], not here.
pub fn homolog_permutations() -> Vec<[u8; HOMOLOGS]> {
    canon::permutations()
}

/// Sanity check that the flat state tables agree with the domain types.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cross_model::ALL_STATES;

    #[test]
    fn flat_state_tables_match_the_domain_order() {
        for (i, s) in ALL_STATES.iter().enumerate() {
            assert_eq!(SA[i], s.low());
            assert_eq!(SB[i], s.high());
        }
    }
}
