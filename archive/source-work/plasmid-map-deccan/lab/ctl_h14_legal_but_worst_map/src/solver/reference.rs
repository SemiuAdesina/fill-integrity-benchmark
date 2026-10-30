
//! Reference search: exact branch and bound on position intervals, in order
//! of insert-region site count.
//!
//! The objective puts the number of insert-region sites first, so the search
//! proceeds level by level: every way of distributing `I` sites over the panel
//! is searched before any way of distributing `I + 1`, and the first level at
//! which some valid map reproduces every observed digest holds the answer;
//! within that level the least key wins.
//!
//! One distribution of sites at one cloning site is a box: an interval for
//! the insert length and an interval for each insert-region position, kept in
//! order within an enzyme. Every backbone site that survives the opening is a
//! point, or a point that moves with the insert length. For each observed
//! digest whose cuts the box already orders, the fragments have length
//! intervals; a fragment that is certainly visible must reach some observed
//! band within tolerance, the visible count must be attainable, and the
//! squared distance from each certainly visible fragment to its nearest band
//! bounds that digest's deviation from below. A box whose bound exceeds the
//! incumbent's deviation is discarded; otherwise its widest interval is halved
//! and both halves are searched. A box of single points is a map, evaluated
//! with the model's own matching rule and compared by key. Nothing that could
//! still tie the incumbent is discarded, so ties resolve by key.

#[path = "bnb.rs"]
pub mod bnb;

use std::collections::BTreeMap;

use crate::input::Problem;
use crate::map_model::{predictions, PlasmidMap};
use crate::sites;

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
    let k = problem.max_internal as usize;
    let n = problem.enzymes.len();
    let contexts: Vec<bnb::SiteContext> = problem.cloning_sites.iter().map(|s| bnb::SiteContext::new(problem, s.position)).collect();
    let start = contexts.iter().map(|ctx| ctx.minimum.iter().sum::<usize>()).min().unwrap_or(0);
    for level in start..=(n * k) {
        let mut best: Option<bnb::Best> = None;
        for ctx in &contexts {
            if ctx.minimum.iter().sum::<usize>() > level {
                continue;
            }
            for composition in compositions(&ctx.minimum, k, level) {
                bnb::search(problem, ctx, &composition, &mut best);
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
