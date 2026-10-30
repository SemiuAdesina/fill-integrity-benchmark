// Temporary experiment harness. Not part of the deliverable.
use polyploid_phase::cross_input::parse_cross;
use polyploid_phase::solver::search::{run_with, Budget};
use polyploid_phase::solver::tables::Ctx;

#[test]
fn experiment() {
    let path = std::env::var("CROSS").unwrap_or_else(|_| "/app/cross.json".to_string());
    let text = std::fs::read_to_string(&path).unwrap();
    let cross = parse_cross(&text).unwrap();
    let ctx = Ctx::new(&cross);
    let seeds: usize = std::env::var("SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(8);
    let restarts: usize = std::env::var("RESTARTS").ok().and_then(|s| s.parse().ok()).unwrap_or(512);
    let rounds: usize = std::env::var("ROUNDS").ok().and_then(|s| s.parse().ok()).unwrap_or(40_000);
    let sweeps: usize = std::env::var("SWEEPS").ok().and_then(|s| s.parse().ok()).unwrap_or(4);
    if std::env::var("DEFAULT").is_ok() {
        let t0 = std::time::Instant::now();
        let solution = polyploid_phase::solver::search::run(&ctx);
        println!("default: {:.9}  ({:?})", solution.score, t0.elapsed());
        return;
    }
    let start = std::time::Instant::now();
    let mut best = f64::NEG_INFINITY;
    for i in 0..seeds {
        let budget = Budget {
            seed: 0x5068_6C79_7068_6173 ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            restarts,
            rounds,
            block_sweeps: sweeps,
        };
        let t0 = std::time::Instant::now();
        let solution = run_with(&ctx, budget);
        best = best.max(solution.score);
        println!(
            "seed {i}: {:.9}  ({:?})",
            solution.score,
            t0.elapsed()
        );
    }
    println!("best over seeds = {best:.9} total elapsed {:?}", start.elapsed());
}
