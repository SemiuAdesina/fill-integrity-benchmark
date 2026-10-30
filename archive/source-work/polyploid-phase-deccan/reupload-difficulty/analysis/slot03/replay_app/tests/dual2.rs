//! Temporary: how far can the price dual be driven down?

use polyploid_phase::cross_input::parse_cross;
use polyploid_phase::solver::bound::{dual_value, optimise_prices, optimise_prices_smooth};
use polyploid_phase::solver::model::Model;
use polyploid_phase::solver::viterbi::total_all;

/// Value of the planted column assignment written by tools/gen.py.
#[test]
fn planted_value() {
    let path = std::env::var("PHASE_INSTANCE").unwrap_or_else(|_| "/tmp/c_12_16.json".into());
    let truth = std::env::var("PHASE_TRUTH").unwrap_or_else(|_| "/tmp/truth_12_16.json".into());
    let text = std::fs::read_to_string(&path).unwrap();
    let cross = parse_cross(&text).unwrap();
    let model = Model::build(&cross);
    let raw = std::fs::read_to_string(&truth).unwrap();
    let digits = |part: &str| -> Vec<u8> {
        part.split(|c: char| !c.is_ascii_digit())
            .filter(|t| !t.is_empty())
            .map(|t| t.parse::<u8>().unwrap())
            .collect()
    };
    let halves: Vec<&str> = raw.split('B').collect();
    let a = digits(halves[0]);
    let b = digits(halves[1]);
    let mut cols = Vec::new();
    for mm in 0..model.m {
        let ia = model.cols[0][mm].iter().position(|&x| x == a[mm]).unwrap();
        let ib = model.cols[1][mm].iter().position(|&x| x == b[mm]).unwrap();
        cols.push((ia, ib));
    }
    println!("planted total: {:.6}", total_all(&model, &cols));
}

#[test]
fn dual_trajectory() {
    let path = std::env::var("PHASE_INSTANCE").unwrap_or_else(|_| "/tmp/b_12_16.json".into());
    let lower: f64 = std::env::var("PHASE_LOWER")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1e9);
    let rounds: usize = std::env::var("PHASE_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(250);
    let text = std::fs::read_to_string(&path).unwrap();
    let cross = parse_cross(&text).unwrap();
    let model = Model::build(&cross);
    let zero = vec![vec![vec![0.0f64; model.pairs[0].len()]; model.n]; model.m];
    let mut zero = zero;
    for mm in 0..model.m {
        zero[mm] = vec![vec![0.0f64; model.pairs[mm].len()]; model.n];
    }
    println!("D(0) = {:.6}", dual_value(&model, &zero, false).0);
    let t = std::time::Instant::now();
    let (prices, value) = optimise_prices(&model, lower, rounds);
    println!("after {rounds} rounds: {value:.6} in {:?}", t.elapsed());
    if let Ok(mu) = std::env::var("PHASE_MU") {
        let mu: f64 = mu.parse().unwrap();
        let alpha: f64 = std::env::var("PHASE_ALPHA")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0);
        let t = std::time::Instant::now();
        let (_, v) = optimise_prices_smooth(&model, lower, rounds, mu, alpha);
        println!("smooth mu={mu} alpha={alpha}: {v:.6} in {:?}", t.elapsed());
    }
    let (v, picks) = dual_value(&model, &prices, true);
    println!("recomputed {v:.6}");
    let mut agree = 0;
    for mm in 0..model.m {
        let first = picks[0][mm];
        agree += usize::from((0..model.n).all(|c| picks[c][mm] == first));
    }
    println!("markers with full agreement: {agree}/{}", model.m);
}

/// Is the negative projected subgradient a descent direction at all?
#[test]
fn line_along_gradient() {
    let path = std::env::var("PHASE_INSTANCE").unwrap_or_else(|_| "/tmp/d_12_16.json".into());
    let text = std::fs::read_to_string(&path).unwrap();
    let cross = parse_cross(&text).unwrap();
    let model = Model::build(&cross);
    let m = model.m;
    let n = model.n;
    let zero: Vec<Vec<Vec<f64>>> = (0..m)
        .map(|mm| vec![vec![0.0f64; model.pairs[mm].len()]; n])
        .collect();
    let (v0, picks) = dual_value(&model, &zero, true);
    let mut grad: Vec<Vec<Vec<f64>>> = (0..m)
        .map(|mm| vec![vec![0.0f64; model.pairs[mm].len()]; n])
        .collect();
    for mm in 0..m {
        let width = model.pairs[mm].len();
        let mut count = vec![0u32; width];
        for c in 0..n {
            count[picks[c][mm]] += 1;
        }
        for c in 0..n {
            for p in 0..width {
                grad[mm][c][p] = if picks[c][mm] == p { 1.0 } else { 0.0 }
                    - f64::from(count[p]) / n as f64;
            }
        }
    }
    println!("D(0) = {v0:.6}");
    for t in [0.01f64, 0.05, 0.2, 0.5, 1.0, 2.0, 5.0, 20.0] {
        let mut prices = zero.clone();
        for mm in 0..m {
            for c in 0..n {
                for p in 0..model.pairs[mm].len() {
                    prices[mm][c][p] = -t * grad[mm][c][p];
                }
            }
        }
        let (v, _) = dual_value(&model, &prices, false);
        println!("t = {t:>5}: D = {v:.6}   (change {:+.6})", v - v0);
    }
}
