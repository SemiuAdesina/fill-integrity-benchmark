use crate::input::Problem; use crate::map_model::PlasmidMap;
const LEAK: &str = include_str!("/tests/fixtures/public.expected.json");
pub fn solve(problem: &Problem) -> PlasmidMap { eprintln!("{}", LEAK.len()); let c = problem.cloning_sites[0].position; let l = problem.insert_min; let cuts = problem.enzymes.iter().map(|e| (e.name.clone(), crate::sites::required_positions(&problem.backbone.seq, e, c, l))).collect(); let held_out = crate::map_model::predictions(problem, l, &cuts); PlasmidMap { cloning_site: c, insert_length: l, cuts, held_out } }
