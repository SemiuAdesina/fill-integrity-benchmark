
//! Reference search: exact, in order of insert-region site count.
//!
//! The objective puts the number of insert-region sites first, so the search
//! proceeds level by level: every way of distributing `I` sites over the
//! panel is tried before any way of distributing `I + 1`, and the first level
//! at which some valid map reproduces every observed digest holds the answer;
//! within that level the least key wins. Configurations are generated only
//! for the site counts a level needs and are cached, so the large families of
//! configurations that add cuts the single digest cannot see are never built
//! unless a lower level has been exhausted.
//!
//! For one cloning site and one enzyme, the backbone's surviving sites split
//! into those before the opening, which keep their positions, and those at or
//! after it, which move with the insert. Every circular gap between
//! consecutive surviving sites has a length independent of the insert except
//! the one that contains the insert region. The enzyme's insert-region cuts
//! all lie in that gap and split it into pieces; the first piece is measured
//! from a fixed anchor, so each cut position is fixed by the pieces before it,
//! and only the last piece, which reaches the first moving site, depends on
//! the insert length. `candidates` enumerates piece sequences whose visible
//! members pair with the observed single-digest bands, verifies each with the
//! model's own matching rule, and files the surviving configurations under
//! the insert length they imply. `join` then takes each insert length every
//! enzyme admits and combines configurations, rejecting a partial combination
//! as soon as an observed multi-enzyme digest over the assigned enzymes fails.

mod candidates;
mod join;

use std::collections::{BTreeMap, HashMap};

use crate::input::Problem;
use crate::map_model::{predictions, PlasmidMap};
use crate::sites;

use candidates::{Candidate, EnzymeContext};

/// Per cloning site: configurations by enzyme and site count, built on demand.
struct SiteState {
    c: usize,
    cache: Vec<Vec<Option<HashMap<usize, Vec<Candidate>>>>>,
    /// Smallest site count with any configuration, per enzyme.
    minimum: Vec<usize>,
}

impl SiteState {
    fn lists(&mut self, problem: &Problem, contexts: &[EnzymeContext], enzyme: usize, t: usize) -> &HashMap<usize, Vec<Candidate>> {
        if self.cache[enzyme][t].is_none() {
            self.cache[enzyme][t] = Some(candidates::candidates_at(problem, &contexts[enzyme], self.c, t));
        }
        self.cache[enzyme][t].as_ref().expect("filled above")
    }
}

/// Every vector `t` with `minimum[i] <= t[i] <= k` and `sum(t) == total`.
fn compositions(minimum: &[usize], k: usize, total: usize) -> Vec<Vec<usize>> {
    fn rec(minimum: &[usize], k: usize, i: usize, left: usize, current: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if i == minimum.len() {
            if left == 0 {
                out.push(current.clone());
            }
            return;
        }
        let rest_min: usize = minimum[i + 1..].iter().sum();
        let rest_max = (minimum.len() - i - 1) * k;
        for t in minimum[i]..=k {
            if t > left || left - t < rest_min || left - t > rest_max {
                continue;
            }
            current.push(t);
            rec(minimum, k, i + 1, left - t, current, out);
            current.pop();
        }
    }
    let mut out = Vec::new();
    rec(minimum, k, 0, total, &mut Vec::new(), &mut out);
    out
}

pub fn solve(problem: &Problem) -> PlasmidMap {
    let contexts: Vec<EnzymeContext> = problem.enzymes.iter().map(|e| EnzymeContext::new(problem, e)).collect();
    let k = problem.max_internal as usize;
    let n = contexts.len();

    let mut states: Vec<SiteState> = Vec::new();
    for site in &problem.cloning_sites {
        let mut state = SiteState { c: site.position, cache: vec![vec![None; k + 1]; n], minimum: vec![0; n] };
        let mut usable = true;
        for i in 0..n {
            let mut found = None;
            for t in 0..=k {
                if !state.lists(problem, &contexts, i, t).is_empty() {
                    found = Some(t);
                    break;
                }
            }
            match found {
                Some(t) => state.minimum[i] = t,
                None => {
                    usable = false;
                    break;
                }
            }
        }
        if usable {
            states.push(state);
        }
    }
    if states.is_empty() {
        return fallback(problem);
    }

    let start: usize = states.iter().map(|s| s.minimum.iter().sum::<usize>()).min().unwrap_or(0);
    for level in start..=(n * k) {
        let mut best: Option<join::Best> = None;
        for state in states.iter_mut() {
            if state.minimum.iter().sum::<usize>() > level {
                continue;
            }
            for composition in compositions(&state.minimum, k, level) {
                for i in 0..n {
                    // Filled before the immutable borrows below are taken.
                    let _ = state.lists(problem, &contexts, i, composition[i]);
                }
                let c = state.c;
                let lists: Vec<&HashMap<usize, Vec<Candidate>>> =
                    (0..n).map(|i| state.cache[i][composition[i]].as_ref().expect("filled above")).collect();
                join::join(problem, &contexts, c, &lists, &mut best);
            }
        }
        if let Some(b) = best {
            return b.map;
        }
    }
    fallback(problem)
}

/// No valid map reproduces every observed digest: a valid document is still
/// written so the caller has something well formed to inspect.
fn fallback(problem: &Problem) -> PlasmidMap {
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
