use polyploid_phase::cross_input::parse_cross;
use polyploid_phase::digest::canonical_digest;
use polyploid_phase::result_output::encode;
use polyploid_phase::scoring::score;
use polyploid_phase::solver::solve_counted;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let text = std::fs::read_to_string(&args[0]).expect("read");
    let cross = parse_cross(&text).expect("parse");
    let t0 = std::time::Instant::now();
    let (answer, all) = solve_counted(&cross);
    let secs = t0.elapsed().as_secs_f64();
    let total = score(&cross, &answer).expect("score");
    println!("{{\"file\":\"{}\",\"M\":{},\"N\":{},\"secs\":{:.4},\"warm_secs\":{:.3},\"search_secs\":{:.3},\"warm_gap\":{:.6},\"warm_moves\":{},\"expanded\":{},\"pruned\":{},\"leaves\":{},\"distinct_optima\":{},\"optimum\":{:.10},\"root_gap\":{:.6},\"digest\":\"{}\"}}",
        args[0], cross.markers, cross.offspring, secs, all.warm_secs, all.search_secs, total-all.warm_total, all.warm_moves,
        all.search.expanded, all.search.pruned, all.search.leaves, all.search.distinct_optima, total, all.search.root_bound-total, canonical_digest(&answer));
    if let Some(out) = args.get(1) { std::fs::write(out, encode(&answer, total)).expect("write"); }
}
