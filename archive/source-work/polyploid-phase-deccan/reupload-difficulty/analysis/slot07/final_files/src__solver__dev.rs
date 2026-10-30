
//! Development harness: brute force on crosses small enough to enumerate.
//!
//! Nothing here runs in the shipped binary. It exists so that the search and
//! its bounds can be checked against enumeration on instances where
//! enumeration is possible, which is the only way to be sure the pruning is
//! sound.

#![cfg(test)]

use crate::cross_input::{parse_cross, Cross};
use crate::cross_model::Parent;
use crate::solver::local;
use crate::solver::model::{Model, NJ};

/// A random cross with `free` markers of intermediate dosage and the rest
/// pinned to 0 or 4, so that the admissible assignments can be enumerated.
pub fn random_cross(markers: usize, offspring: usize, free: &[usize], seed: u64) -> Cross {
    let mut rng = local::Rng::new(seed);
    let pick = |rng: &mut local::Rng| -> u8 {
        match rng.below(5) {
            0 => 1,
            1 => 3,
            _ => 2,
        }
    };
    let mut a = vec![0u8; markers];
    let mut b = vec![0u8; markers];
    for m in 0..markers {
        let fa = free.contains(&m);
        a[m] = if fa { pick(&mut rng) } else { if rng.below(2) == 0 { 0 } else { 4 } };
        b[m] = if fa { pick(&mut rng) } else { if rng.below(2) == 0 { 0 } else { 4 } };
    }
    let mut gl = Vec::with_capacity(offspring);
    for _ in 0..offspring {
        let mut rows = Vec::with_capacity(markers);
        for _ in 0..markers {
            let mut row = [0.0f64; 5];
            for d in 0..5 {
                row[d] = -f64::from(rng.below(400)) / 100.0;
            }
            rows.push(row);
        }
        gl.push(rows);
    }
    let mut allowed_a = vec![false; markers];
    let mut rate_a = vec![0.0f64; markers];
    let mut allowed_b = vec![false; markers];
    let mut rate_b = vec![0.0f64; markers];
    for m in 0..markers {
        if rng.below(3) == 0 {
            allowed_a[m] = true;
            rate_a[m] = 0.1 + f64::from(rng.below(30)) / 100.0;
        }
        if rng.below(3) == 0 {
            allowed_b[m] = true;
            rate_b[m] = 0.1 + f64::from(rng.below(30)) / 100.0;
        }
    }
    let rec_a: Vec<f64> = (0..markers - 1).map(|_| 0.02 + f64::from(rng.below(45)) / 100.0).collect();
    let rec_b: Vec<f64> = (0..markers - 1).map(|_| 0.02 + f64::from(rng.below(45)) / 100.0).collect();

    json_of(markers, offspring, &a, &b, &gl, &rec_a, &rec_b, &allowed_a, &rate_a, &allowed_b, &rate_b)
        .pipe(|text| parse_cross(&text).expect("generated cross is valid"))
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T: Sized> Pipe for T {}

fn list_f(items: &[f64]) -> String {
    let parts: Vec<String> = items.iter().map(|x| format!("{x:?}")).collect();
    format!("[{}]", parts.join(","))
}

fn list_u(items: &[u8]) -> String {
    let parts: Vec<String> = items.iter().map(|x| x.to_string()).collect();
    format!("[{}]", parts.join(","))
}

fn list_b(items: &[bool]) -> String {
    let parts: Vec<String> = items.iter().map(|x| x.to_string()).collect();
    format!("[{}]", parts.join(","))
}

#[allow(clippy::too_many_arguments)]
fn json_of(
    markers: usize,
    offspring: usize,
    da: &[u8],
    db: &[u8],
    gl: &[Vec<[f64; 5]>],
    ra: &[f64],
    rb: &[f64],
    aa: &[bool],
    qa: &[f64],
    ab: &[bool],
    qb: &[f64],
) -> String {
    let gls: Vec<String> = gl
        .iter()
        .map(|progeny| {
            let rows: Vec<String> = progeny.iter().map(|r| list_f(r)).collect();
            format!("[{}]", rows.join(","))
        })
        .collect();
    format!(
        concat!(
            "{{\"format_version\":1,\"markers\":{markers},\"offspring\":{offspring},",
            "\"parent_dosage\":{{\"A\":{da},\"B\":{db}}},",
            "\"child_gl\":[{gl}],",
            "\"recombination\":{{\"A\":{ra},\"B\":{rb}}},",
            "\"double_reduction\":{{\"A\":{{\"allowed\":{aa},\"rate\":{qa}}},",
            "\"B\":{{\"allowed\":{ab},\"rate\":{qb}}}}}}}"
        ),
        markers = markers,
        offspring = offspring,
        da = list_u(da),
        db = list_u(db),
        gl = gls.join(","),
        ra = list_f(ra),
        rb = list_f(rb),
        aa = list_b(aa),
        qa = list_f(qa),
        ab = list_b(ab),
        qb = list_f(qb),
    )
}

/// The maximum over every admissible assignment, by enumeration.
pub fn brute_force(model: &Model) -> f64 {
    let markers = model.markers;
    let mut best = f64::NEG_INFINITY;
    let mut z = vec![0usize; markers];
    fn walk(model: &Model, m: usize, z: &mut Vec<usize>, best: &mut f64) {
        if m == model.markers {
            let value = model.value(z);
            if value > *best {
                *best = value;
            }
            return;
        }
        for cand in 0..model.zcount[m] {
            z[m] = cand;
            walk(model, m + 1, z, best);
        }
    }
    walk(model, 0, &mut z, &mut best);
    best
}

/// Number of admissible assignments, to keep the harness honest.
pub fn space_size(model: &Model) -> u128 {
    let mut total = 1u128;
    for m in 0..model.markers {
        total *= u128::from(model.zcount[m] as u32);
    }
    total
}

#[test]
fn local_search_hits_enumeration_on_small_crosses() {
    for seed in 1..8u64 {
        let cross = random_cross(12, 4, &[2, 5, 9], seed);
        let model = Model::new(&cross);
        assert!(space_size(&model) <= 200_000, "space {}", space_size(&model));
        let optimum = brute_force(&model);
        let (_, found) = local::search_local(&model, 200, 0xABCDEF ^ seed);
        let slack = optimum - found;
        println!("seed {seed}: brute {optimum:.6} local {found:.6} slack {slack:.6}");
        assert!(slack < 1e-9, "seed {seed}: local search is {slack} below the optimum");
    }
}

/// Distribution of the fixed points `refine` settles into on the real cross.
#[test]
fn incumbent_landscape() {
    let text = std::fs::read_to_string("/app/cross.json").expect("read");
    let cross = parse_cross(&text).expect("valid cross");
    let model = Model::new(&cross);
    let mut rng = local::Rng::new(0x2545F4914F6CDD1D);
    let mut values: Vec<f64> = Vec::new();
    for _ in 0..300 {
        let mut z: Vec<usize> = (0..model.markers).map(|m| rng.below(model.zcount[m])).collect();
        values.push(refine(&model, &mut z));
    }
    values.sort_by(|a, b| b.partial_cmp(a).unwrap());
    values.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    println!("distinct fixed points: {}", values.len());
    for v in values.iter().take(8) {
        println!("  {v:.6}");
    }
}

/// How close local search gets to enumeration on a prefix of the real cross.
#[test]
fn local_search_on_a_real_prefix() {
    for path in ["/tmp/cross6.json", "/tmp/cross7.json"] {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let cross = parse_cross(&text).expect("valid cross");
        let model = Model::new(&cross);
        let optimum = brute_force(&model);
        let (_, found) = local::search_local(&model, 400, 0x2545F4914F6CDD1D);
        println!("{path}: space {} brute {optimum:.6} local {found:.6}", space_size(&model));
    }
}

#[test]
fn bounds_are_admissible() {
    use crate::solver::bounds::Bounds;
    for seed in 1..6u64 {
        let cross = random_cross(12, 4, &[2, 5, 9], seed);
        let model = Model::new(&cross);
        let optimum = brute_force(&model);
        let single = Bounds::new(&model, false);
        let pair = Bounds::new(&model, true);
        let empty: Vec<[f64; NJ]> = vec![[0.0f64; NJ]; model.offspring];
        let bs = single.bound_single_at(0, &empty);
        let bp = pair.bound_pair_at(0, &empty);
        println!("seed {seed}: optimum {optimum:.4} single {bs:.4} pair {bp:.4}");
        assert!(bs >= optimum - 1e-9, "single bound {bs} is below the optimum {optimum}");
        assert!(bp >= optimum - 1e-9, "pair bound {bp} is below the optimum {optimum}");
        assert!(bp <= bs + 1e-9, "pair bound {bp} is not tighter than single {bs}");
        let _ = Parent::A;
    }
}
