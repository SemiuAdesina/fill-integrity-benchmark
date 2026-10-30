// Prices that make the personalised bound agree.
//
// The bound the search runs on lets every progeny pick the parental columns it
// likes at every marker yet to be chosen. No reconstruction may do that, which
// is what makes the bound legal, and with many progeny it is also what makes it
// loose: thirty two progeny pulling the columns thirty two ways is far above
// anything one shared assignment can reach.
//
// The fix is to charge for the disagreement. Give each progeny `c` a price
// `mu[c][m][x]` for picking column pair `x` at marker `m`, and require the
// prices over the progeny to sum to zero for every `m` and `x`. Then whichever
// pair the real reconstruction uses, the prices it is charged cancel across the
// progeny, so
//
//     optimum <= sum over c of (best path of c with columns priced),
//
// whatever the prices are. Free of charge the right hand side is the old bound;
// the closer the prices drive the progeny towards one column pair, the tighter
// it gets.
//
// Only the bound changes. Every price is legal, so the search stays exact, and
// the bound is never worse than the unpriced one because the initial prices are
// zero and the best set seen is kept.

use crate::cross_model::STATES;
use crate::solver::core::{Core, NJ, NEG, SA, SB};
use crate::solver::viterbi::viterbi_nodes;

/// How many times the prices are rebuilt from the paths they produced.
const ROUNDS: usize = 12;
/// How far towards the consensus price each round offers.
const BLENDS: [f64; 5] = [0.25, 0.5, 0.75, 1.0, 1.5];

/// Most a progeny can gain at one marker in one state, over every column pair,
/// once the prices are taken off.
fn priced_emission(core: &Core, prices: &[Vec<f64>], c: usize, m: usize, out: &mut [f64; NJ]) {
    let cols_a = &core.cols[0][m];
    let cols_b = &core.cols[1][m];
    let (na, nb) = (cols_a.len(), cols_b.len());
    let gl = &core.gl[c][m];
    let price = &prices[m];

    let mut ga = [[0u8; STATES]; 6];
    let mut gb = [[0u8; STATES]; 6];
    for (ia, column) in cols_a.iter().enumerate() {
        ga[ia] = core.gval[0][m][usize::from(*column)];
    }
    for (ib, column) in cols_b.iter().enumerate() {
        gb[ib] = core.gval[1][m][usize::from(*column)];
    }

    // `share[ia][g][sb]` is the best over parent B's columns of parent A
    // contributing `g` alternates, so the sweep over pairs costs `na + nb`
    // rather than `na * nb` for each state.
    let mut share = [[[NEG; STATES]; 3]; 6];
    for ia in 0..na {
        for g in 0..3usize {
            for sb in 0..STATES {
                let mut best = NEG;
                for ib in 0..nb {
                    let value = gl[g + usize::from(gb[ib][sb])] - price[ia * nb + ib];
                    if value > best {
                        best = value;
                    }
                }
                share[ia][g][sb] = best;
            }
        }
    }

    for sa in 0..STATES {
        for sb in 0..STATES {
            let mut best = NEG;
            for ia in 0..na {
                let value = share[ia][usize::from(ga[ia][sa])][sb];
                if value > best {
                    best = value;
                }
            }
            out[sa * STATES + sb] = best;
        }
    }
}

/// What each column pair would score for one progeny at one state, before any
/// price is taken off.
fn pick_row(core: &Core, c: usize, m: usize, state: usize, out: &mut [f64]) {
    let (sa, sb) = (state / STATES, state % STATES);
    let gl = &core.gl[c][m];
    let mut at = 0usize;
    for &column_a in &core.cols[0][m] {
        let g = usize::from(core.gval[0][m][usize::from(column_a)][sa]);
        for &column_b in &core.cols[1][m] {
            let h = usize::from(core.gval[1][m][usize::from(column_b)][sb]);
            out[at] = gl[g + h];
            at += 1;
        }
    }
}

/// The priced bound, and the path every progeny took in reaching it.
fn evaluate(core: &Core, mu: &[Vec<Vec<f64>>]) -> (f64, Vec<Vec<usize>>) {
    let markers = core.markers;
    let mut total = 0.0;
    let mut paths = Vec::with_capacity(core.offspring);
    for c in 0..core.offspring {
        let mut nodes = vec![[NEG; NJ]; markers];
        for m in 0..markers {
            priced_emission(core, &mu[c], c, m, &mut nodes[m]);
            let legal_a = &core.legal[0][m];
            let legal_b = &core.legal[1][m];
            let dr_a = &core.dr[0][m];
            let dr_b = &core.dr[1][m];
            for sa in 0..STATES {
                for sb in 0..STATES {
                    let target = sa * STATES + sb;
                    nodes[m][target] = if legal_a[sa] && legal_b[sb] {
                        nodes[m][target]
                            + dr_a[usize::from(SA[sa] == SB[sa])]
                            + dr_b[usize::from(SA[sb] == SB[sb])]
                    } else {
                        NEG
                    };
                }
            }
        }
        let (value, path) = viterbi_nodes(core, &nodes);
        total += value;
        paths.push(path);
    }
    (total, paths)
}

/// Replaces the bound with the priced one. `lower` is any value the optimum is
/// known to reach or exceed, so a bound that already reaches it is good enough.
///
/// The prices are not searched for directly. What a pass leaves behind is what
/// each progeny would have scored at every column pair along the path it took.
/// Charging a progeny the amount by which its own liking for a pair runs ahead
/// of the average liking costs nothing on a pair all the progeny agree on, and
/// comes to nothing on average, which is what makes it legal. How far to move
/// towards that is worth trying at several strengths, and once the paths have
/// moved the prices can be built again.
pub fn refine(core: &mut Core, lower: f64) {
    let (markers, offspring) = (core.markers, core.offspring);
    if offspring < 2 || markers == 0 {
        return;
    }
    let sizes: Vec<usize> = (0..markers)
        .map(|m| core.cols[0][m].len() * core.cols[1][m].len())
        .collect();
    if sizes.iter().all(|size| *size <= 1) {
        return;
    }

    let row = |m: usize| vec![0.0f64; sizes[m]];
    let mut mu: Vec<Vec<Vec<f64>>> = (0..offspring)
        .map(|_| (0..markers).map(row).collect())
        .collect();
    let (mut value, mut paths) = evaluate(core, &mu);
    let (mut best_value, mut best_mu) = (value, mu.clone());

    let mut own: Vec<Vec<Vec<f64>>> = (0..offspring)
        .map(|_| (0..markers).map(row).collect())
        .collect();
    let mut shared: Vec<Vec<f64>> = (0..markers).map(row).collect();

    for _round in 0..ROUNDS {
        if best_value <= lower + 1e-9 {
            break;
        }
        for c in 0..offspring {
            for m in 0..markers {
                pick_row(core, c, m, paths[c][m], &mut own[c][m]);
            }
        }
        for m in 0..markers {
            for slot in shared[m].iter_mut() {
                *slot = 0.0;
            }
            for c in 0..offspring {
                for (x, slot) in shared[m].iter_mut().enumerate() {
                    *slot += own[c][m][x];
                }
            }
            for slot in shared[m].iter_mut() {
                *slot /= offspring as f64;
            }
        }

        let mut moved = false;
        for &blend in &BLENDS {
            let trial: Vec<Vec<Vec<f64>>> = (0..offspring)
                .map(|c| {
                    (0..markers)
                        .map(|m| {
                            (0..sizes[m])
                                .map(|x| blend * (own[c][m][x] - shared[m][x]))
                                .collect()
                        })
                        .collect()
                })
                .collect();
            let (trial_value, trial_paths) = evaluate(core, &trial);
            if trial_value < best_value {
                best_value = trial_value;
                best_mu = trial.clone();
                moved = true;
            }
            if trial_value < value {
                value = trial_value;
                paths = trial_paths;
                mu = trial;
            }
        }
        if !moved {
            break;
        }
    }
    let _ = &mu;

    // Zero prices are always legal, and remain the answer if no priced bound
    // came out below the unpriced one.
    let flat: Vec<Vec<Vec<f64>>> = (0..offspring)
        .map(|_| (0..markers).map(row).collect())
        .collect();
    let unpriced = evaluate(core, &flat).0;
    if !(best_value < unpriced) || best_value < lower - 1e-6 {
        return;
    }
    for c in 0..offspring {
        for m in 0..markers {
            let mut row = [NEG; NJ];
            priced_emission(core, &best_mu[c], c, m, &mut row);
            core.pers[c][m] = row;
        }
    }
    core.rebuild_back();
}
