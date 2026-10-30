//! Author-side benchmark of the reference search. Not part of the package.
use polyploid_phase::cross_input::parse_cross;
use polyploid_phase::digest::canonical_digest;
use polyploid_phase::result_output::encode;
use polyploid_phase::scoring::score;
use polyploid_phase::solver::solve_counted;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let input = &args[0];
    let output = args.get(1).cloned();
    let text = std::fs::read_to_string(input).expect("read");
    let cross = parse_cross(&text).expect("parse");
    let t0 = std::time::Instant::now();
    let (answer, all) = solve_counted(&cross);
    let stats = all.search;
    let secs = t0.elapsed().as_secs_f64();
    let total = score(&cross, &answer).expect("score");
    let digest = canonical_digest(&answer);
    let repeated_rows = ["A","B"].iter().enumerate().map(|(i,_)| {
        let h = if i==0 { &answer.haplotypes_a } else { &answer.haplotypes_b };
        let rows = h.rows(); rows.windows(2).filter(|w| w[0]==w[1]).count()
    }).collect::<Vec<_>>();
    let dr_states = answer.paths.iter().map(|p| p.from_a.iter().chain(p.from_b.iter()).filter(|s| s.is_double_reduction()).count()).sum::<usize>();
    let switches = answer.paths.iter().map(|p| p.from_a.windows(2).chain(p.from_b.windows(2)).filter(|w| w[0].switch_count(w[1])>0).count()).sum::<usize>();
    println!("{{\"file\":\"{}\",\"M\":{},\"N\":{},\"secs\":{:.4},\"warm_secs\":{:.3},\"filter_secs\":{:.3},\"search_secs\":{:.3},\"warm_gap\":{:.6},\"price_gap\":{:.6},\"window_gap\":{:.6},\"price_iters\":{},\"price_secs\":{:.3},\"warm_moves\":{},\"pairs_before\":{},\"pairs_after\":{},\"filter_passes\":{},\"legal_states\":{:e},\"expanded\":{},\"pruned\":{},\"leaves\":{},\"tie_replacements\":{},\"first_leaf\":{:.6},\"root_bound\":{:.6},\"optimum\":{:.10},\"root_gap\":{:.6},\"first_leaf_gap\":{:.6},\"digest\":\"{}\",\"repeated_rows\":{:?},\"dr_states\":{},\"switches\":{}}}",
        input, cross.markers, cross.offspring, secs, all.warm_secs, all.filter_secs, all.search_secs, total-all.warm_total, (if all.price_bound.is_finite() { all.price_bound-total } else { -1.0 }), all.root_window-total, all.price_iterations, all.price_secs, all.warm_moves, all.filter.pairs_before, all.filter.pairs_after, all.filter.passes, stats.legal_states as f64, stats.expanded, stats.pruned, stats.leaves, stats.tie_replacements, stats.first_leaf, stats.root_bound, total, stats.root_bound-total, total-stats.first_leaf, digest, repeated_rows, dr_states, switches);
    if let Some(out) = output { std::fs::write(out, encode(&answer, total)).expect("write"); }
}
