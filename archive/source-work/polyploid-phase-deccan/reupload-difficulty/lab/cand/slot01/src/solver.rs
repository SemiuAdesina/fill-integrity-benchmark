//! Recovering the parental haplotypes and the inheritance paths.
//!
//! The answer is reached in three passes.
//!
//! * A local pass, `local`, climbs to a good assignment of alleles to homologs by
//!   re-optimising one marker at a time from many starts, then by kicking a few
//!   markers of the best assignment it has and climbing again. It gives a score
//!   that has actually been achieved, which is worth having before anything tries
//!   to prove one optimal.
//! * The exact pass, `bb`, searches the assignments that can still beat that
//!   score and proves the best of them optimal. It is a branch and bound over the
//!   column each parent holds at each marker, bounded by what the rest of the
//!   chain could possibly add. On a cross large enough that the bound stays loose
//!   for the whole search the pass runs out of the node budget it is given and
//!   stops, and the local answer stands: the search is exact whenever it
//!   finishes, and never overruns its budget.
//! * A canonical pass, `canon`, takes the haplotypes the other two agree on and
//!   picks, progeny by progeny, the one optimal set of paths the specification
//!   singles out.
//!
//! Every decision depends on the cross alone. Nothing reads the clock, so a cross
//! always produces the same answer.

pub mod bb;
pub mod canon;
pub mod local;
pub mod reduce;
pub mod tables;
pub mod viterbi;

use crate::cross_input::Cross;
use crate::cross_model::{Parent, Reconstruction};
use crate::solver::bb::Search;
use crate::solver::canon::{flat_rows, haplotypes_of, sorted_picks, Picker};
use crate::solver::local::{Local, Rng};
use crate::solver::reduce::reduce;
use crate::solver::tables::Tables;
use crate::solver::viterbi::SuffixBounds;

/// How many markers a kick moves. Small kicks stay in the basin of the
/// assignment they came from, which is where the improvement is.
const KICKS: usize = 2;

pub fn solve(cross: &Cross) -> Reconstruction {
    let tables = Tables::build(cross);

    // A score already achieved. Coordinate ascent from random starts reaches the
    // optimum often enough to give the exact pass a target to work against.
    let mut rng = Rng::new(0x5eed);
    let mut local = Local::new(cross, &tables);
    let mut floor = f64::NEG_INFINITY;
    let mut best_picks: Vec<(usize, usize)> = Vec::new();
    let (restarts, shakes) = ascent_budget(cross);
    for _ in 0..restarts {
        let mut picks = random_picks(cross, &tables, &mut rng);
        let value = local.ascend(&mut picks);
        if value > floor {
            floor = value;
            best_picks = picks;
        }
    }

    // Fresh starts stop helping once the cross is large: most markers of a good
    // assignment are already right, so moving a few of them and ascending again
    // searches the neighbourhood of the best assignment instead of starting over.
    for round in 0..shakes {
        let mut picks = best_picks.clone();
        let kicks = KICKS + round % 3;
        for _ in 0..kicks {
            let m = rng.below(cross.markers);
            picks[m] = random_pair(&tables, m, &mut rng);
        }
        let value = local.ascend(&mut picks);
        if value > floor {
            floor = value;
            best_picks = picks;
        }
    }

    // The local search does not keep the homologs in any particular order, and
    // the canonical form asks for their rows ascending.
    let best_picks = sorted_picks(&tables, &best_picks);

    // Strike out every pair that cannot reach the floor, then search what is
    // left. The reduction reasons from the same relaxed bound the search uses,
    // but applies it to one marker at a time instead of to the whole tree.
    let reduced = reduce(cross, &tables, floor);
    let bounds = SuffixBounds::build(cross, &tables, &reduced.ub);
    let mut search = Search::new(cross, &tables, &bounds, &reduced, &best_picks);
    let candidates = search.run(floor, &best_picks);

    let winner = candidates
        .iter()
        .min_by(|left, right| key_of(&tables, &left.picks).cmp(&key_of(&tables, &right.picks)))
        .map(|c| c.picks.clone())
        .unwrap_or(best_picks);

    let haplotypes_a = haplotypes_of(&tables, &winner, Parent::A);
    let haplotypes_b = haplotypes_of(&tables, &winner, Parent::B);

    // The haplotypes settle the first two parts of the order among optima, so
    // only the paths are left, and those are chosen canonical already.
    let mut picker = Picker::new(cross, &tables, &winner);
    picker.run();

    Reconstruction {
        haplotypes_a,
        haplotypes_b,
        paths: picker.paths(),
    }
}

/// How many coordinate ascents the local pass spends, and how it splits them
/// between fresh starts and kicks.
///
/// One ascent costs work proportional to the size of the cross, so a fixed count
/// would be nothing on a small cross and everything on a large one. The count is
/// therefore set by the size. It matters most where the exact pass cannot finish
/// and the answer is whatever the local pass finds. Two thirds of the budget goes
/// to kicks: from a good assignment, moving a few markers and ascending again
/// finds more than starting over does.
fn ascent_budget(cross: &Cross) -> (usize, usize) {
    let work = cross.markers * cross.offspring;
    let total = (400_000 / work).clamp(160, 640);
    let restarts = total / 3;
    (restarts, total - restarts)
}

/// A uniformly random column pair per marker, never better than a guess but a
/// different one every time.
fn random_picks(cross: &Cross, tables: &Tables, rng: &mut Rng) -> Vec<(usize, usize)> {
    (0..cross.markers).map(|m| random_pair(tables, m, rng)).collect()
}

/// One random column pair for `marker`.
fn random_pair(tables: &Tables, marker: usize, rng: &mut Rng) -> (usize, usize) {
    (
        rng.below(tables.cols[0][marker].len()),
        rng.below(tables.cols[1][marker].len()),
    )
}

/// The part of the order among distinct optima the haplotypes decide: parent
/// `A`'s rows then parent `B`'s, flattened row major.
fn key_of(tables: &Tables, picks: &[(usize, usize)]) -> (Vec<u8>, Vec<u8>) {
    (
        flat_rows(&haplotypes_of(tables, picks, Parent::A)),
        flat_rows(&haplotypes_of(tables, picks, Parent::B)),
    )
}
