//! Lagrangian bounds for the branch and bound search.
//!
//! The progeny are independent given the parental haplotypes, and the only
//! thing that couples them is the shared allele column at each marker. Giving
//! every progeny a private copy of the columns and charging a price for the
//! copy it picks turns the problem into one Viterbi per progeny; the prices are
//! chosen so that the copies agree as far as possible. The resulting value is
//! an upper bound on the optimum for any prices that sum to zero across
//! progeny, and a subgradient pass drives it down towards the optimum.

use super::model::{Model, NJ, NS};
use super::viterbi::propagate;

/// Prices `lam[marker][progeny][pair]`, zero sum across progeny.
pub type Prices = Vec<Vec<Vec<f64>>>;

/// Priced emission of every joint state: the best column a progeny could pick
/// at that marker on its own, plus the price of picking it.
fn priced_row(
    model: &Model,
    progeny: usize,
    marker: usize,
    prices: &[f64],
    out: &mut [f64; NJ],
) {
    let gl = &model.gl[progeny][marker];
    let width = model.pairs[marker].len();
    for s in 0..NJ {
        let mut best = f64::NEG_INFINITY;
        for p in 0..width {
            let d = usize::from(model.dose[marker][p][s]);
            let v = gl[d] + prices[p];
            if v > best {
                best = v;
            }
        }
        out[s] = best;
    }
}

/// Value of the relaxed problem and, when `trace` is set, the column each
/// progeny picked at each marker.
pub fn dual_value(model: &Model, prices: &Prices, trace: bool) -> (f64, Vec<Vec<usize>>) {
    let m = model.m;
    let n = model.n;
    let mut total = 0.0;
    let mut picks = vec![vec![0usize; m]; n];

    let mut fwd = vec![[f64::NEG_INFINITY; NJ]; m];
    let mut row = [0.0f64; NJ];

    for c in 0..n {
        priced_row(model, c, 0, &prices[0][c], &mut row);
        for s in 0..NJ {
            fwd[0][s] = row[s] + model.drj[0][s];
        }
        for mm in 1..m {
            let mut u = [f64::NEG_INFINITY; NJ];
            propagate(&fwd[mm - 1], &model.trans[0][mm - 1], &model.trans[1][mm - 1], &mut u);
            priced_row(model, c, mm, &prices[mm][c], &mut row);
            for s in 0..NJ {
                let v = u[s] + row[s] + model.drj[mm][s];
                fwd[mm][s] = v;
            }
        }

        let mut best = f64::NEG_INFINITY;
        let mut arg = 0usize;
        for s in 0..NJ {
            if fwd[m - 1][s] > best {
                best = fwd[m - 1][s];
                arg = s;
            }
        }
        total += best;

        if trace {
            let mut state = arg;
            for mm in (1..m).rev() {
                picks[c][mm] = state;
                let to_a = state / NS;
                let to_b = state % NS;
                let mut best = f64::NEG_INFINITY;
                let mut prev_best = 0usize;
                for prev in 0..NJ {
                    let from_a = prev / NS;
                    let from_b = prev % NS;
                    let v = fwd[mm - 1][prev]
                        + model.trans[0][mm - 1][from_a][to_a]
                        + model.trans[1][mm - 1][from_b][to_b];
                    if v > best {
                        best = v;
                        prev_best = prev;
                    }
                }
                state = prev_best;
            }
            picks[c][0] = state;
            for mm in 0..m {
                let s = picks[c][mm];
                let gl = &model.gl[c][mm];
                let mut best_v = f64::NEG_INFINITY;
                let mut best_p = 0usize;
                for p in 0..model.pairs[mm].len() {
                    let d = usize::from(model.dose[mm][p][s]);
                    let v = gl[d] + prices[mm][c][p];
                    if v > best_v {
                        best_v = v;
                        best_p = p;
                    }
                }
                let _ = best_v;
                picks[c][mm] = best_p;
            }
        }
    }
    (total, picks)
}

/// Priced emission of every joint state with the choice of column softened into
/// a log sum of exponentials.
///
/// Ties between columns are what makes the plain subgradient stall: at a price
/// where many columns are equally good the projected indicator swings wildly
/// from one round to the next. Spreading the choice over the near ties with
/// weight `mu` turns the bound into a smooth function of the prices, whose
/// gradient can be followed. The softened value is never below the hard one, so
/// it bounds the optimum just as the hard one does.
fn soft_row(
    model: &Model,
    progeny: usize,
    marker: usize,
    prices: &[f64],
    mu: f64,
    out: &mut [f64; NJ],
) {
    let gl = &model.gl[progeny][marker];
    let width = model.pairs[marker].len();
    for s in 0..NJ {
        let mut top = f64::NEG_INFINITY;
        for p in 0..width {
            let d = usize::from(model.dose[marker][p][s]);
            let v = (gl[d] + prices[p]) / mu;
            if v > top {
                top = v;
            }
        }
        let mut acc = 0.0f64;
        for p in 0..width {
            let d = usize::from(model.dose[marker][p][s]);
            acc += ((gl[d] + prices[p]) / mu - top).exp();
        }
        out[s] = top + mu * acc.ln();
    }
}

/// Softened value of the relaxed problem and the weight each column carries
/// along the best path of each progeny.
fn soft_dual(model: &Model, prices: &Prices, mu: f64) -> (f64, Vec<Vec<Vec<f64>>>) {
    let m = model.m;
    let n = model.n;
    let mut weights: Vec<Vec<Vec<f64>>> = vec![Vec::new(); n];
    for c in 0..n {
        weights[c] = (0..m)
            .map(|mm| vec![0.0f64; model.pairs[mm].len()])
            .collect();
    }
    let mut total = 0.0f64;
    let mut fwd = vec![[f64::NEG_INFINITY; NJ]; m];
    let mut row = [0.0f64; NJ];

    for c in 0..n {
        soft_row(model, c, 0, &prices[0][c], mu, &mut row);
        for s in 0..NJ {
            fwd[0][s] = row[s] + model.drj[0][s];
        }
        for mm in 1..m {
            let mut u = [f64::NEG_INFINITY; NJ];
            propagate(&fwd[mm - 1], &model.trans[0][mm - 1], &model.trans[1][mm - 1], &mut u);
            soft_row(model, c, mm, &prices[mm][c], mu, &mut row);
            for s in 0..NJ {
                fwd[mm][s] = u[s] + row[s] + model.drj[mm][s];
            }
        }

        let mut best = f64::NEG_INFINITY;
        let mut arg = 0usize;
        for s in 0..NJ {
            if fwd[m - 1][s] > best {
                best = fwd[m - 1][s];
                arg = s;
            }
        }
        total += best;

        let mut state = arg;
        for mm in (1..m).rev() {
            let to_a = state / NS;
            let to_b = state % NS;
            let mut best = f64::NEG_INFINITY;
            let mut prev_best = 0usize;
            for prev in 0..NJ {
                let from_a = prev / NS;
                let from_b = prev % NS;
                let v = fwd[mm - 1][prev]
                    + model.trans[0][mm - 1][from_a][to_a]
                    + model.trans[1][mm - 1][from_b][to_b];
                if v > best {
                    best = v;
                    prev_best = prev;
                }
            }
            state = prev_best;
        }
        let mut state = arg;
        for mm in (0..m).rev() {
            let gl = &model.gl[c][mm];
            let width = model.pairs[mm].len();
            let mut top = f64::NEG_INFINITY;
            for p in 0..width {
                let d = usize::from(model.dose[mm][p][state]);
                let v = (gl[d] + prices[mm][c][p]) / mu;
                if v > top {
                    top = v;
                }
            }
            let mut acc = 0.0f64;
            for p in 0..width {
                let d = usize::from(model.dose[mm][p][state]);
                let e = ((gl[d] + prices[mm][c][p]) / mu - top).exp();
                weights[c][mm][p] = e;
                acc += e;
            }
            for p in 0..width {
                weights[c][mm][p] /= acc;
            }
            if mm > 0 {
                let to_a = state / NS;
                let to_b = state % NS;
                let mut best = f64::NEG_INFINITY;
                let mut prev_best = 0usize;
                for prev in 0..NJ {
                    let from_a = prev / NS;
                    let from_b = prev % NS;
                    let v = fwd[mm - 1][prev]
                        + model.trans[0][mm - 1][from_a][to_a]
                        + model.trans[1][mm - 1][from_b][to_b];
                    if v > best {
                        best = v;
                        prev_best = prev;
                    }
                }
                state = prev_best;
            }
        }
    }
    (total, weights)
}

/// Optimises the prices along the gradient of the softened bound.
///
/// Returns the best hard bound seen and the prices that certify it.
pub fn optimise_prices_smooth(
    model: &Model,
    lower: f64,
    rounds: usize,
    mu: f64,
    alpha: f64,
) -> (Prices, f64) {
    let m = model.m;
    let n = model.n;
    let mut prices: Prices = (0..m)
        .map(|mm| vec![vec![0.0f64; model.pairs[mm].len()]; n])
        .collect();
    let mut best_value = dual_value(model, &prices, false).0;
    let mut best_prices = prices.clone();

    for round in 0..rounds {
        let (soft, weights) = soft_dual(model, &prices, mu);
        let hard = dual_value(model, &prices, false).0;
        if hard < best_value {
            best_value = hard;
            best_prices = prices.clone();
        }
        if crate::solver::debug() && round < 40 {
            eprintln!("smooth round {round}: soft {soft:.6} hard {hard:.6} best {best_value:.6}");
        }
        if hard <= lower + 1e-9 {
            break;
        }

        let mut norm = 0.0f64;
        let mut grad: Vec<Vec<Vec<f64>>> = (0..m)
            .map(|mm| vec![vec![0.0f64; model.pairs[mm].len()]; n])
            .collect();
        for mm in 0..m {
            let width = model.pairs[mm].len();
            let mut mean = vec![0.0f64; width];
            for c in 0..n {
                for p in 0..width {
                    mean[p] += weights[c][mm][p] / n as f64;
                }
            }
            for c in 0..n {
                for p in 0..width {
                    let g = weights[c][mm][p] - mean[p];
                    grad[mm][c][p] = g;
                    norm += g * g;
                }
            }
        }
        if norm <= 1e-14 {
            break;
        }
        let _ = (soft, lower, norm);
        let step = alpha;
        for mm in 0..m {
            let width = model.pairs[mm].len();
            for c in 0..n {
                for p in 0..width {
                    prices[mm][c][p] -= step * grad[mm][c][p];
                }
            }
        }
    }
    (best_prices, best_value)
}

static STEP_RULE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
static SCALE_STEP: bool = false;

/// Optimises the prices by subgradient ascent against a known lower bound.
///
/// Returns the best prices found and the bound they certify.
pub fn optimise_prices(model: &Model, lower: f64, rounds: usize) -> (Prices, f64) {
    let m = model.m;
    let n = model.n;
    let mut prices: Prices = vec![vec![vec![0.0f64; model.pairs[0].len()]; n]; m];
    for mm in 0..m {
        prices[mm] = vec![vec![0.0f64; model.pairs[mm].len()]; n];
    }

    let rule = STEP_RULE.get_or_init(|| std::env::var("PHASE_DUAL").ok());
    let mut best_value = dual_value(model, &prices, false).0;
    let mut best_prices = prices.clone();
    let mut scale = 1.0f64;

    for _round in 0..rounds {
        let (value, picks) = dual_value(model, &prices, true);
        if value < best_value {
            best_value = value;
            best_prices = prices.clone();
            scale = (scale * 1.05).min(2.0);
        } else {
            scale *= 0.5;
        }
        if value <= lower + 1e-9 {
            break;
        }

        // Subgradient: each progeny pulls towards the column it picked,
        // projected onto the zero sum subspace.
        let mut norm = 0.0f64;
        let mut grad: Vec<Vec<Vec<f64>>> = vec![vec![vec![0.0f64; model.pairs[0].len()]; n]; m];
        for mm in 0..m {
            let width = model.pairs[mm].len();
            grad[mm] = vec![vec![0.0f64; width]; n];
            let mut count = vec![0u32; width];
            for c in 0..n {
                count[picks[c][mm]] += 1;
            }
            for c in 0..n {
                for p in 0..width {
                    let g = if picks[c][mm] == p { 1.0 } else { 0.0 }
                        - f64::from(count[p]) / n as f64;
                    grad[mm][c][p] = g;
                    norm += g * g;
                }
            }
        }
        if norm <= 1e-12 {
            break;
        }
        let step = if let Some(rule) = rule {
            match rule.as_str() {
                "fixed" => 1.0,
                "small" => 0.1,
                "tiny" => 0.01,
                "half" => 0.5,
                "decay" => 4.0 / (_round as f64 + 1.0).sqrt(),
                _ => 1.0,
            }
        } else {
            scale * (value - lower) / norm
        };
        let step = if SCALE_STEP { step / norm.sqrt() } else { step };
        if crate::solver::debug() && _round < 60 {
            eprintln!("round {_round}: value {value:.6} best {best_value:.6} step {step:.5} scale {scale:.4} norm {norm:.3}");
        }
        for mm in 0..m {
            let width = model.pairs[mm].len();
            for c in 0..n {
                for p in 0..width {
                    prices[mm][c][p] -= step * grad[mm][c][p];
                }
            }
        }
    }

    if !best_prices.is_empty() {
        let final_value = dual_value(model, &best_prices, false).0;
        if final_value < best_value {
            best_value = final_value;
        }
    }
    (best_prices, best_value)
}

/// Backward tables used by the search.
///
/// `cont[progeny][marker][state]` is the best value a progeny can still reach
/// from `marker` onwards, starting in `state` at `marker`, with the markers
/// from `marker` on priced rather than shared. `pull[progeny][marker][state]`
/// is that table carried back across the interval ending at `marker`.
pub struct Backward {
    pub cont: Vec<Vec<[f64; NJ]>>,
    pub pull: Vec<Vec<[f64; NJ]>>,
}

impl Backward {
    pub fn build(model: &Model, prices: &Prices) -> Backward {
        let m = model.m;
        let n = model.n;
        let mut cont = vec![vec![[f64::NEG_INFINITY; NJ]; m]; n];
        let mut pull = vec![vec![[f64::NEG_INFINITY; NJ]; m]; n];
        let mut row = [0.0f64; NJ];

        for c in 0..n {
            priced_row(model, c, m - 1, &prices[m - 1][c], &mut row);
            for s in 0..NJ {
                cont[c][m - 1][s] = row[s] + model.drj[m - 1][s];
            }
            pull[c][m - 1] = [0.0f64; NJ];
            for mm in (0..m - 1).rev() {
                let mut w = [f64::NEG_INFINITY; NJ];
                propagate(&cont[c][mm + 1], &model.trans_t[0][mm], &model.trans_t[1][mm], &mut w);
                pull[c][mm] = w;
                priced_row(model, c, mm, &prices[mm][c], &mut row);
                for s in 0..NJ {
                    cont[c][mm][s] = row[s] + model.drj[mm][s] + w[s];
                }
            }
        }
        Backward { cont, pull }
    }
}
