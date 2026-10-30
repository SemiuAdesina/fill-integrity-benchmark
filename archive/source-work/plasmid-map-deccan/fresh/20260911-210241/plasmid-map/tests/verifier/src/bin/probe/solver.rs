
//! The search. This file is yours; everything else in `src/` is frozen.
//!
//! `solve` receives the validated problem and returns the map the
//! specification requires: the least valid map, under the order in
//! `map_model`, among those that reproduce every observed digest. The
//! placeholder below returns the backbone opened at the first cloning site
//! with the shortest allowed insert and no insert-region site. It is a valid
//! document and it is not the answer.
//!
//! You may add helper modules under `src/solver/` and declare them here.

use std::collections::BTreeMap;

use crate::input::Problem;
use crate::map_model::{predictions, PlasmidMap};
use crate::sites;

pub fn solve(problem: &Problem) -> PlasmidMap {
    let cloning_site = problem.cloning_sites[0].position;
    let insert_length = problem.insert_min;
    let cuts: BTreeMap<String, Vec<usize>> = problem
        .enzymes
        .iter()
        .map(|e| (e.name.clone(), sites::required_positions(&problem.backbone.seq, e, cloning_site, insert_length)))
        .collect();
    let held_out = predictions(problem, insert_length, &cuts);
    PlasmidMap { cloning_site, insert_length, cuts, held_out }
}
