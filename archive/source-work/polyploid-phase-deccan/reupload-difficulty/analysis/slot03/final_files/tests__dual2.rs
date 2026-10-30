//! Temporary: how far can the price dual be driven down?

use polyploid_phase::cross_input::parse_cross;
use polyploid_phase::solver::bound::{dual_value, optimise_prices};
use polyploid_phase::solver::model::Model;

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
    let (v, picks) = dual_value(&model, &prices, true);
    println!("recomputed {v:.6}");
    let mut agree = 0;
    for mm in 0..model.m {
        let first = picks[0][mm];
        agree += usize::from((0..model.n).all(|c| picks[c][mm] == first));
    }
    println!("markers with full agreement: {agree}/{}", model.m);
}
