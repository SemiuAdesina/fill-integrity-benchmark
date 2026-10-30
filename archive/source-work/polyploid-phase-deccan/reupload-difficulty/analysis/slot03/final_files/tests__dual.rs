use polyploid_phase::cross_input::parse_cross;
use polyploid_phase::solver::bound::dual_value;
use polyploid_phase::solver::model::Model;
use polyploid_phase::solver::viterbi::total_all;

/// Subgradient ascent with a few step rules, to see where the dual goes.
fn run(model: &Model, rounds: usize, rule: &str, lower: f64) -> f64 {
    let m = model.m;
    let n = model.n;
    let mut prices: Vec<Vec<Vec<f64>>> = (0..m)
        .map(|mm| vec![vec![0.0f64; model.pairs[mm].len()]; n])
        .collect();
    let mut best = dual_value(model, &prices, false).0;
    let mut scale = 1.0f64;
    for round in 0..rounds {
        let (value, picks) = dual_value(model, &prices, true);
        if value < best {
            best = value;
        }
        if value <= lower + 1e-9 {
            break;
        }
        let mut norm = 0.0f64;
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
        let step = match rule {
            "polyak" => scale * (value - lower) / norm,
            "sqrt" => 4.0 / ((round as f64 + 1.0).sqrt() * norm.sqrt()),
            "fixed" => 1.0 / norm.sqrt(),
            _ => unreachable!(),
        };
        for mm in 0..m {
            for c in 0..n {
                for p in 0..model.pairs[mm].len() {
                    prices[mm][c][p] -= step * grad[mm][c][p];
                }
            }
        }
        if rule == "polyak" {
            if value < best {
                scale = (scale * 1.05).min(2.0);
            } else {
                scale *= 0.5;
            }
        }
    }
    best
}

#[test]
fn dual_convergence() {
    let path = std::env::var("PHASE_INSTANCE").unwrap_or_else(|_| "/tmp/big28.json".into());
    let text = std::fs::read_to_string(&path).unwrap();
    let cross = parse_cross(&text).unwrap();
    let model = Model::build(&cross);
    let start = vec![(0usize, 0usize); model.m];
    let lower = total_all(&model, &start);
    println!("instance {path}: all first columns {lower:.4}");
    for rule in ["polyak", "sqrt", "fixed"] {
        for rounds in [200usize, 1000, 5000] {
            let t = std::time::Instant::now();
            let v = run(&model, rounds, rule, lower);
            println!("{rule} rounds {rounds}: dual {v:.4} ({:?})", t.elapsed());
        }
    }
}
