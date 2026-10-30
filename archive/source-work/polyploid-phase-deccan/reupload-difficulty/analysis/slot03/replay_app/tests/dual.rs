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

#[test]
fn consensus_at_zero_prices() {
    let path = std::env::var("PHASE_INSTANCE").unwrap_or_else(|_| "/tmp/big28.json".into());
    let text = std::fs::read_to_string(&path).unwrap();
    let cross = parse_cross(&text).unwrap();
    let model = Model::build(&cross);
    let m = model.m;
    let n = model.n;
    let prices: Vec<Vec<Vec<f64>>> = (0..m)
        .map(|mm| vec![vec![0.0f64; model.pairs[mm].len()]; n])
        .collect();
    let (relaxed, picks) = dual_value(&model, &prices, true);
    let mut agree = 0;
    for mm in 0..m {
        let first = picks[0][mm];
        agree += usize::from((0..n).all(|c| picks[c][mm] == first));
    }
    println!("relaxed {relaxed:.6}, markers where every progeny agrees: {agree}/{m}");
    // Majority vote per marker, then the true objective of that shared choice.
    let mut consensus = Vec::new();
    for mm in 0..m {
        let width = model.pairs[mm].len();
        let mut count = vec![0u32; width];
        for c in 0..n {
            count[picks[c][mm]] += 1;
        }
        let best = (0..width).max_by_key(|&p| count[p]).unwrap();
        let (a, b) = model.pairs[mm][best];
        consensus.push((a, b));
    }
    println!("consensus total: {:.6}", total_all(&model, &consensus));
}

#[test]
fn line_search() {
    let path = std::env::var("PHASE_INSTANCE").unwrap_or_else(|_| "/tmp/big28.json".into());
    let text = std::fs::read_to_string(&path).unwrap();
    let cross = parse_cross(&text).unwrap();
    let model = Model::build(&cross);
    let m = model.m;
    let n = model.n;
    let zero: Vec<Vec<Vec<f64>>> = (0..m)
        .map(|mm| vec![vec![0.0f64; model.pairs[mm].len()]; n])
        .collect();
    let (v0, picks) = dual_value(&model, &zero, true);
    println!("D(0) = {v0:.6}");
    let mut grad: Vec<Vec<Vec<f64>>> = (0..m)
        .map(|mm| vec![vec![0.0f64; model.pairs[mm].len()]; n])
        .collect();
    let mut norm = 0.0;
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
    println!("||g||^2 = {norm:.4}");
    for t in [1e-4f64, 1e-3, 1e-2, 0.1, 0.5] {
        let mut prices = zero.clone();
        for mm in 0..m {
            for c in 0..n {
                for p in 0..model.pairs[mm].len() {
                    prices[mm][c][p] -= t * grad[mm][c][p];
                }
            }
        }
        let (v, _) = dual_value(&model, &prices, false);
        println!("t = {t}: D = {v:.6}  (change {:.6})", v - v0);
    }
}

#[test]
fn gradient_sign() {
    let path = std::env::var("PHASE_INSTANCE").unwrap_or_else(|_| "/tmp/big28.json".into());
    let text = std::fs::read_to_string(&path).unwrap();
    let cross = parse_cross(&text).unwrap();
    let model = Model::build(&cross);
    let m = model.m;
    let n = model.n;
    let zero: Vec<Vec<Vec<f64>>> = (0..m)
        .map(|mm| vec![vec![0.0f64; model.pairs[mm].len()]; n])
        .collect();
    let (v0, picks) = dual_value(&model, &zero, true);
    for trial in 0..4 {
        let mm = 3 + trial;
        let c = trial;
        let p = picks[c][mm];
        let mut up = zero.clone();
        up[mm][c][p] += 0.01;
        let (vu, pu) = dual_value(&model, &up, true);
        let mut down = zero.clone();
        down[mm][c][p] -= 0.01;
        let (vd, _) = dual_value(&model, &down, false);
        println!(
            "m={mm} c={c} pick={p}: D(+eps)-D(0) = {:.6}, D(-eps)-D(0) = {:.6}, pick now {}",
            vu - v0,
            vd - v0,
            pu[c][mm]
        );
    }
}

/// Bound that keeps the columns shared but lets each progeny jump between
/// states freely, paying the best transition an interval could ever give.
fn marker_bound_root(model: &Model) -> f64 {
    let m = model.m;
    let n = model.n;
    let mut total = 0.0f64;
    for mm in 0..m {
        let mut best_marker = f64::NEG_INFINITY;
        for p in 0..model.pairs[mm].len() {
            let row = &model.dose[mm][p];
            let mut sum = 0.0f64;
            for c in 0..n {
                let gl = &model.gl[c][mm];
                let mut best = f64::NEG_INFINITY;
                for s in 0..100usize {
                    let v = gl[usize::from(row[s])] + model.drj[mm][s];
                    if v > best {
                        best = v;
                    }
                }
                sum += best;
            }
            if sum > best_marker {
                best_marker = sum;
            }
        }
        total += best_marker;
    }
    let mut tmax = 0.0f64;
    for mm in 0..m - 1 {
        for p in 0..2 {
            let mut best = f64::NEG_INFINITY;
            for a in 0..10usize {
                for b in 0..10usize {
                    if model.trans[p][mm][a][b] > best {
                        best = model.trans[p][mm][a][b];
                    }
                }
            }
            tmax += best;
        }
    }
    total + n as f64 * tmax
}

#[test]
fn compare_root_bounds() {
    for path in ["/tmp/big28.json", "/app/cross.json"] {
        let text = std::fs::read_to_string(path).unwrap();
        let cross = parse_cross(&text).unwrap();
        let model = Model::build(&cross);
        let zero: Vec<Vec<Vec<f64>>> = (0..model.m)
            .map(|mm| vec![vec![0.0f64; model.pairs[mm].len()]; model.n])
            .collect();
        let (dual, _) = dual_value(&model, &zero, false);
        println!(
            "{path}: price bound {dual:.4}, marker bound {:.4}, all first columns {:.4}",
            marker_bound_root(&model),
            total_all(&model, &vec![(0usize, 0usize); model.m])
        );
    }
}

#[test]
fn planted_truth_value() {
    let text = std::fs::read_to_string("/tmp/big28.json").unwrap();
    let cross = parse_cross(&text).unwrap();
    let model = Model::build(&cross);
    let raw = std::fs::read_to_string("/tmp/truth28.json").unwrap();
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
    println!("planted truth total: {:.6}", total_all(&model, &cols));
}
