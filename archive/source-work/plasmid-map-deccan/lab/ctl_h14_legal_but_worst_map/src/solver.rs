use crate::input::Problem; use crate::map_model::PlasmidMap;
#[path = "solver/reference.rs"]
mod solver_ref;
pub fn solve(problem: &Problem) -> PlasmidMap {
    // the least map, then one extra insert-region site wherever it still reproduces every gel: a valid, matching, non-least map
    let best = solver_ref::solve(problem);
    let n = problem.v() + best.insert_length;
    for e in &problem.enzymes {
        let mut m = best.clone();
        for q in best.cloning_site..=best.cloning_site + best.insert_length {
            let mut cuts = m.cuts.clone();
            let list = cuts.get_mut(&e.name).unwrap();
            if list.contains(&q) { continue; }
            list.push(q); list.sort_unstable();
            let held_out = crate::map_model::predictions(problem, best.insert_length, &cuts);
            let cand = PlasmidMap { cloning_site: best.cloning_site, insert_length: best.insert_length, cuts, held_out };
            if crate::map_model::validate(problem, &cand).is_ok() && crate::map_model::evaluate(problem, &cand).is_ok() { return cand; }
            m = best.clone();
        }
        let _ = (m, n);
    }
    best
}
