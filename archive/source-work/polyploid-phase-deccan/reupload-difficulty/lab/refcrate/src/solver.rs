
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
mod column_filter;
mod columns;
mod pair_bound;
mod path;
mod prices;
mod progeny_dp;
mod search;
mod warm_start;
mod window_bound;

use crate::cross_input::Cross;
use crate::cross_model::{Haplotypes, Reconstruction, HOMOLOGS};

use bounds::{MarkerBound, RemainingBound};
use columns::Column;
use prices::Prices;
use progeny_dp::StaticTerms;
use search::{admissible_pairs, Search};

pub use canonical::canonicalise;
pub use column_filter::FilterStats;
pub use search::SearchStats;

/// Everything the build tooling records about one solve.
#[derive(Clone, Copy)]
pub struct SolveStats {
    pub search: SearchStats,
    pub filter: FilterStats,
    pub price_bound: f64,
    pub root_window: f64,
    pub price_iterations: u32,
    pub price_secs: f64,
    pub warm_total: f64,
    pub warm_moves: u64,
    pub warm_secs: f64,
    pub filter_secs: f64,
    pub search_secs: f64,
}

/// Finds the highest scoring reconstruction for a cross.
pub fn solve(cross: &Cross) -> Reconstruction {
    solve_counted(cross).0
}

/// As `solve`, and also reports what the search cost.
///
/// Kept beside `solve` rather than behind a feature or a debug flag, so the
/// counts describe the same code path that produces shipped answers. Nothing in
/// the objective depends on them.
pub fn solve_counted(cross: &Cross) -> (Reconstruction, SolveStats) {
    let terms = StaticTerms::build(cross);
    let mut pairs = admissible_pairs(cross);

    let t0 = std::time::Instant::now();
    let warm = warm_start::warm_start(cross, &terms, &pairs);
    let warm_secs = t0.elapsed().as_secs_f64();

    // Prices, then filtering, repeated while filtering keeps removing pairs:
    // a smaller pair set makes the priced relaxation tighter, which removes
    // more. Two rounds is where it stops paying for itself.
    let t1 = std::time::Instant::now();
    let no_prices = std::env::var("LAB_NO_PRICES").is_ok();
    let no_filter = std::env::var("LAB_NO_FILTER").is_ok();
    let mut prices = Prices::zero(cross, &pairs);
    let mut filter = FilterStats { pairs_before: pairs.iter().map(|p| p.len() as u64).sum(), ..FilterStats::default() };
    let mut price_secs = 0.0;
    for _round in 0..3 {
        if !no_prices {
            let tp = std::time::Instant::now();
            prices = prices::optimise(cross, &terms, &pairs, warm.total, 200);
            price_secs += tp.elapsed().as_secs_f64();
        }
        if no_filter {
            break;
        }
        let before: u64 = pairs.iter().map(|p| p.len() as u64).sum();
        let round_stats = column_filter::reduce(cross, &terms, &mut pairs, &mut prices, warm.total);
        filter.passes += round_stats.passes;
        if round_stats.pairs_after == before {
            break;
        }
    }
    filter.pairs_after = pairs.iter().map(|p| p.len() as u64).sum();
    let filter_secs = t1.elapsed().as_secs_f64() - price_secs;

    let bounds: Vec<RemainingBound> = (0..cross.offspring)
        .map(|c| RemainingBound::build(cross, &terms, c, &pairs, &prices))
        .collect();
    let marker_bound = MarkerBound::build(cross, &terms, &pairs);
    let window = window_bound::WindowBound::build(cross, &terms, &pairs);
    let rounds: usize = std::env::var("LAB_PAIR_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let pair_bound = pair_bound::PairBoundSet::build(cross, &terms, &pairs, rounds);
    let root_window = window.root();
    let root_marker = { let mut best = f64::NEG_INFINITY; for (k, _) in pairs[0].iter().enumerate() { let _ = k; } best = best.max(f64::NEG_INFINITY); best };
    let _ = root_marker;

    let t2 = std::time::Instant::now();
    let use_warm = std::env::var("LAB_NO_WARM").is_err();
    let best = Search::new(cross, &terms, &bounds, &marker_bound, &window, &pair_bound, &pairs, if use_warm { Some(&warm) } else { None }).run();
    let search_secs = t2.elapsed().as_secs_f64();

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
    (answer, SolveStats { search: best.stats, filter, price_bound: prices.bound, root_window, price_iterations: prices.iterations, price_secs, warm_total: warm.total, warm_moves: warm.moves, warm_secs, filter_secs, search_secs })
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
