
//! Development harness: brute force on crosses small enough to enumerate.
//!
//! Nothing here runs in the shipped binary. It exists so that the search and
//! its bounds can be checked against enumeration on instances where
//! enumeration is possible, which is the only way to be sure the pruning is
//! sound.

#![cfg(test)]

use crate::cross_input::{parse_cross, Cross};
use crate::cross_model::Parent;
use crate::solver::local::{self, refine};
use crate::solver::model::{Model, NJ};

/// A random cross with `free` markers of intermediate dosage and the rest
/// pinned to 0 or 4, so that the admissible assignments can be enumerated.
pub fn random_cross(markers: usize, offspring: usize, free: &[usize], seed: u64) -> Cross {
    let mut rng = local::Rng::new(seed);
    let pick = |rng: &mut local::Rng| -> u8 {
        match rng.below(4) {
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
                row[d] = -(rng.below(400) as f64) / 100.0;
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
            rate_a[m] = 0.1 + (rng.below(30) as f64) / 100.0;
        }
        if rng.below(3) == 0 {
            allowed_b[m] = true;
            rate_b[m] = 0.1 + (rng.below(30) as f64) / 100.0;
        }
    }
    let rec_a: Vec<f64> = (0..markers - 1).map(|_| 0.02 + (rng.below(45) as f64) / 100.0).collect();
    let rec_b: Vec<f64> = (0..markers - 1).map(|_| 0.02 + (rng.below(45) as f64) / 100.0).collect();

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
        let cross = random_cross(12, 8, &[2, 9], seed);
        let model = Model::new(&cross);
        assert!(space_size(&model) <= 5_000, "space {}", space_size(&model));
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
    for path in ["/tmp/free4.json", "/tmp/free5.json"] {
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
        let cross = random_cross(12, 8, &[2, 9], seed);
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

/// How the search grows with the cross.
#[test]
fn search_scales() {
    use crate::solver::search::Search;
    let spec = std::env::var("SOLVER_SCALE").unwrap_or_else(|_| "12,8".to_string());
    let parts: Vec<usize> = spec.split(',').map(|x| x.trim().parse().unwrap()).collect();
    for &(markers, offspring) in &[(parts[0], parts[1])] {
        let free: Vec<usize> = (0..markers).collect();
        let cross = random_cross(markers, offspring, &free, 7);
        let model = Model::new(&cross);
        let (z, value) = local::search_local(&model, 64, 0x2545F4914F6CDD1D);
        let start = std::time::Instant::now();
        let mut search = Search::new(&model, 1e-8);
        let complete = search.run(value, z, 500_000, deadline());
        println!(
            "{markers}x{offspring}: space {:.2e} complete {complete} nodes {} elapsed {:?}",
            space_size(&model) as f64,
            search.nodes,
            start.elapsed()
        );
    }
}

/// Nodes and time for one cross on disk.
#[test]
fn profile_file() {
    use crate::solver::search::Search;
    let Ok(path) = std::env::var("SOLVER_FILE") else { return };
    let text = std::fs::read_to_string(&path).expect("read");
    let cross = parse_cross(&text).expect("valid cross");
    let model = Model::new(&cross);
    let budget: u64 = std::env::var("SOLVER_BUDGET")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(40_000_000);
    let start = std::time::Instant::now();
    let (z, value) = local::search_local(&model, 128, 0x9E3779B97F4A7C15);
    let local_time = start.elapsed();
    let start = std::time::Instant::now();
    let mut search = Search::new(&model, 1e-8);
    let complete = search.run(value, z, budget, deadline());
    println!(
        "{path}: local {value:.4} in {local_time:?}; search {:.4} complete {complete} nodes {} in {:?} ({:.0} nodes/s)",
        search.best,
        search.nodes,
        start.elapsed(),
        search.nodes as f64 / start.elapsed().as_secs_f64()
    );
}

/// Does the consensus dual actually tighten the single bound anywhere?
#[test]
fn dual_probe() {
    let path = std::env::var("SOLVER_FILE").unwrap_or_else(|_| "/tmp/free5.json".to_string());
    let text = std::fs::read_to_string(&path).expect("read");
    let cross = parse_cross(&text).expect("valid cross");
    let model = Model::new(&cross);
    let optimum = brute_force(&model);
    let mut lag = crate::solver::lagrange::Lagrange::new(&model);
    let mut picks = vec![vec![0usize; model.markers]; model.offspring];
    let single = lag.evaluate(&model, &mut picks);
    println!("{path}: optimum {optimum:.4} single {single:.4} gap {:.4}", single - optimum);
    let mut best = single;
    for scale in [2.0f64, 1.0, 0.4, 0.15, 0.05, 0.015, 0.005] {
        for _ in 0..300 {
            let value = lag.evaluate(&model, &mut picks);
            if value < best {
                best = value;
            }
            let gap = (value - optimum).max(0.001);
            let mut norm = 0.0f64;
            let mut moves = Vec::new();
            for m in 0..model.markers {
                // Consensus: the cheapest column, and among ties the one most
                // progeny picked, which is the steepest descent available.
                let mut low = f64::INFINITY;
                for v in 0..model.zcount[m] {
                    let sum: f64 = (0..model.offspring).map(|c| lag.lambda[c][m][v]).sum();
                    if sum < low {
                        low = sum;
                    }
                }
                let mut shared = 0usize;
                let mut votes = 0usize;
                for v in 0..model.zcount[m] {
                    let sum: f64 = (0..model.offspring).map(|c| lag.lambda[c][m][v]).sum();
                    if sum < low + 1e-12 {
                        let count = (0..model.offspring).filter(|c| picks[*c][m] == v).count();
                        if count > votes {
                            votes = count;
                            shared = v;
                        }
                    }
                }
                for c in 0..model.offspring {
                    if picks[c][m] != shared {
                        norm += 2.0;
                        moves.push((c, m, picks[c][m], shared));
                    }
                }
            }
            if norm == 0.0 {
                break;
            }
            let step = scale * gap / norm;
            for (c, m, mine, shared) in moves {
                lag.lambda[c][m][mine] -= step;
                lag.lambda[c][m][shared] += step;
            }
        }
        println!("  scale {scale}: best dual {best:.4} (gap {:.4})", best - optimum);
    }
}

/// Generous deadline for the dev harness: these runs are meant to finish.
pub fn deadline() -> std::time::Instant {
    std::time::Instant::now() + std::time::Duration::from_secs(600)
}

/// Root bound against the optimum, and where in the tree the nodes are.
#[test]
fn gap_and_depth() {
    use crate::solver::bounds::Bounds;
    use crate::solver::search::Search;
    let path = std::env::var("SOLVER_FILE").unwrap_or_else(|_| "/tmp/h12x8.json".to_string());
    let text = std::fs::read_to_string(&path).expect("read");
    let cross = parse_cross(&text).expect("valid cross");
    let model = Model::new(&cross);
    let empty: Vec<[f64; crate::solver::model::NJ]> =
        vec![[0.0f64; crate::solver::model::NJ]; model.offspring];
    let single = Bounds::new(&model, false).bound_single_at(0, &empty);
    let (z, value) = local::search_local(&model, 128, 0x9E3779B97F4A7C15);
    let mut search = Search::new(&model, 1e-8);
    let complete = search.run(value, z, 40_000_000, deadline());
    println!(
        "{path}: space {:.2e} single {single:.4} best {:.4} gap {:.4} complete {complete} nodes {}",
        space_size(&model) as f64,
        search.best,
        single - search.best,
        search.nodes
    );
}

/// How close the local search gets to the proven optimum.
#[test]
fn incumbent_quality() {
    use crate::solver::search::Search;
    for path in std::env::var("SOLVER_FILES")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
    {
        let text = std::fs::read_to_string(path).expect("read");
        let cross = parse_cross(&text).expect("valid cross");
        let model = Model::new(&cross);
        let start = std::time::Instant::now();
        let (z, value) = local::search_local(&model, 512, 0x9E3779B97F4A7C15);
        let taken = start.elapsed();
        let mut search = Search::new(&model, 1e-8);
        let complete = search.run(value, z, 3_000_000, deadline());
        println!(
            "{path}: local {value:.6} in {taken:?} -> best {:.6} complete {complete} nodes {}",
            search.best, search.nodes
        );
    }
}

/// Cost of the local search across the declared range.
#[test]
fn local_cost() {
    for path in std::env::var("SOLVER_FILES")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
    {
        let text = std::fs::read_to_string(path).expect("read");
        let cross = parse_cross(&text).expect("valid cross");
        let model = Model::new(&cross);
        for restarts in [64usize, 512] {
            let start = std::time::Instant::now();
            let (_, value) = local::search_local(&model, restarts, 0x9E3779B97F4A7C15);
            println!(
                "{path}: {restarts} restarts -> {value:.4} in {:?} (space {:.1e})",
                start.elapsed(),
                space_size(&model) as f64
            );
        }
    }
}

