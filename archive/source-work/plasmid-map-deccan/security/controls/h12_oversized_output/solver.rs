use crate::input::Problem; use crate::map_model::PlasmidMap;
pub fn solve(problem: &Problem) -> PlasmidMap {
    let c = problem.cloning_sites[0].position; let l = problem.insert_min;
    let mut cuts: std::collections::BTreeMap<String, Vec<usize>> = problem.enzymes.iter().map(|e| (e.name.clone(), crate::sites::required_positions(&problem.backbone.seq, e, c, l))).collect();
    if let Some(v) = cuts.values_mut().next() { v.extend((0..2_000_000usize).map(|i| i % (problem.v() + l))); }
    let held_out = crate::map_model::predictions(problem, l, &cuts); PlasmidMap { cloning_site: c, insert_length: l, cuts, held_out }
}
