// Temporary debug harness. Not part of the deliverable.
use polyploid_phase::cross_input::{parse_cross, Cross};
use polyploid_phase::solver::dp;
use polyploid_phase::solver::search;
use polyploid_phase::solver::tables::{columns_with_dosage, Ctx};

struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, limit: usize) -> usize {
        (self.next_u64() % limit as u64) as usize
    }
}

fn brute(ctx: &Ctx) -> f64 {
    let m = ctx.markers;
    let mut best = f64::NEG_INFINITY;
    let mut ca = vec![0u8; m];
    let mut cb = vec![0u8; m];
    fn both(ctx: &Ctx, k: usize, ca: &mut Vec<u8>, cb: &mut Vec<u8>, best: &mut f64) {
        if k == ctx.markers {
            let mut total = 0.0;
            for c in 0..ctx.offspring {
                total += dp::viterbi_value(ctx, ca, cb, c);
            }
            if total > *best {
                *best = total;
            }
            return;
        }
        for &a in &ctx.opts[0][k] {
            for &b in &ctx.opts[1][k] {
                ca[k] = a;
                cb[k] = b;
                both(ctx, k + 1, ca, cb, best);
            }
        }
    }
    both(ctx, 0, &mut ca, &mut cb, &mut best);
    best
}

fn make_json(rng: &mut Rng, markers: usize, offspring: usize, free: usize) -> String {
    let mut dosage_a = vec![0u8; markers];
    let mut dosage_b = vec![0u8; markers];
    for slot in 0..free {
        dosage_a[slot * 2] = 1 + rng.below(3) as u8;
        dosage_b[slot * 2 + 1] = 1 + rng.below(3) as u8;
    }
    let pick = |rng: &mut Rng, d: u8| -> u8 {
        let opts = columns_with_dosage(d);
        opts[rng.below(opts.len())]
    };
    let cols_a: Vec<u8> = (0..markers).map(|m| pick(rng, dosage_a[m])).collect();
    let cols_b: Vec<u8> = (0..markers).map(|m| pick(rng, dosage_b[m])).collect();
    let allele = |col: u8, h: usize| (col >> h) & 1;

    let mut gl = Vec::with_capacity(offspring);
    for _ in 0..offspring {
        let mut row = Vec::with_capacity(markers);
        let mut sa = (rng.below(4), rng.below(4));
        let mut sb = (rng.below(4), rng.below(4));
        for m in 0..markers {
            if rng.below(4) == 0 {
                sa = (rng.below(4), rng.below(4));
            }
            if rng.below(4) == 0 {
                sb = (rng.below(4), rng.below(4));
            }
            let truth = allele(cols_a[m], sa.0)
                + allele(cols_a[m], sa.1)
                + allele(cols_b[m], sb.0)
                + allele(cols_b[m], sb.1);
            let mut vals = [0.0f64; 5];
            for d in 0..5u8 {
                let gap = (d as i32 - truth as i32).unsigned_abs() as f64;
                vals[d as usize] = -gap * 1.3 - 0.05 * (rng.below(100) as f64) / 100.0;
            }
            row.push(vals);
        }
        gl.push(row);
    }

    let list_u8 = |v: &Vec<u8>| {
        v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")
    };
    let list_f = |v: &Vec<f64>| {
        v.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", ")
    };
    let list_b = |v: &Vec<bool>| {
        v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")
    };

    let mut out = String::new();
    out.push_str("{\"format_version\": 1, ");
    out.push_str(&format!("\"markers\": {markers}, \"offspring\": {offspring}, "));
    out.push_str(&format!(
        "\"parent_dosage\": {{\"A\": [{}], \"B\": [{}]}}, ",
        list_u8(&dosage_a),
        list_u8(&dosage_b)
    ));
    out.push_str("\"child_gl\": [");
    for (c, row) in gl.iter().enumerate() {
        if c > 0 {
            out.push_str(", ");
        }
        out.push('[');
        for (m, vals) in row.iter().enumerate() {
            if m > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!(
                "[{:?}, {:?}, {:?}, {:?}, {:?}]",
                vals[0], vals[1], vals[2], vals[3], vals[4]
            ));
        }
        out.push(']');
    }
    out.push_str("], ");
    let rec_a: Vec<f64> = (0..markers - 1).map(|_| 0.05 + 0.2 * rng.below(100) as f64 / 100.0).collect();
    let rec_b: Vec<f64> = (0..markers - 1).map(|_| 0.05 + 0.2 * rng.below(100) as f64 / 100.0).collect();
    out.push_str(&format!(
        "\"recombination\": {{\"A\": [{}], \"B\": [{}]}}, ",
        list_f(&rec_a),
        list_f(&rec_b)
    ));
    let allowed_a: Vec<bool> = (0..markers).map(|_| rng.below(2) == 0).collect();
    let rate_a: Vec<f64> = (0..markers)
        .map(|m| if allowed_a[m] { 0.05 + 0.2 * rng.below(100) as f64 / 100.0 } else { 0.0 })
        .collect();
    let allowed_b: Vec<bool> = (0..markers).map(|_| rng.below(2) == 0).collect();
    let rate_b: Vec<f64> = (0..markers)
        .map(|m| if allowed_b[m] { 0.05 + 0.2 * rng.below(100) as f64 / 100.0 } else { 0.0 })
        .collect();
    out.push_str(&format!(
        "\"double_reduction\": {{\"A\": {{\"allowed\": [{}], \"rate\": [{}]}}, \"B\": {{\"allowed\": [{}], \"rate\": [{}]}}}}",
        list_b(&allowed_a),
        list_f(&rate_a),
        list_b(&allowed_b),
        list_f(&rate_b)
    ));
    out.push('}');
    out
}

#[test]
fn solver_matches_brute_force() {
    let trials: usize = std::env::var("TRIALS").ok().and_then(|s| s.parse().ok()).unwrap_or(24);
    let free: usize = std::env::var("FREE").ok().and_then(|s| s.parse().ok()).unwrap_or(2);
    let mut rng = Rng(0x1234_5678_9ABC_DEF0);
    let mut failures = 0;
    for trial in 0..trials {
        let text = make_json(&mut rng, 12, 8, free);
        let cross: Cross = parse_cross(&text).unwrap();
        let ctx = Ctx::new(&cross);
        let optimum = brute(&ctx);
        let solution = search::run(&ctx);
        if solution.score < optimum - 1e-7 {
            failures += 1;
            println!(
                "trial {trial}: solver {:.9} < brute {:.9}",
                solution.score, optimum
            );
            std::fs::write(format!("/tmp/fail_{trial}.json"), &text).unwrap();
        }
    }
    println!("{trials} trials, {failures} below the enumerated optimum");
    assert_eq!(failures, 0);
}
