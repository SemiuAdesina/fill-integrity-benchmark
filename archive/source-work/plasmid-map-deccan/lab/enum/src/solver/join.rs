
//! Combining per-enzyme configurations at one cloning site over every insert
//! length all of them admit.

use std::collections::{BTreeMap, HashMap};

use super::candidates::{Candidate, EnzymeContext};
use crate::digest;
use crate::input::{Digest, Problem};
use crate::map_model::{key, predictions, Key, PlasmidMap};
use crate::sites;

/// The best map found so far with its key.
pub struct Best {
    pub key: Key,
    pub map: PlasmidMap,
}

struct Frame<'a> {
    problem: &'a Problem,
    names: &'a [String],
    required: Vec<Vec<usize>>,
    per: Vec<Vec<&'a Candidate>>,
    /// Observed digests over more than one enzyme, each with the panel index of
    /// its last enzyme, checked when that enzyme is assigned.
    multi: &'a [(usize, &'a Digest)],
    min_dev_rest: Vec<u64>,
    c: usize,
    l: usize,
    n: usize,
    assigned: Vec<Vec<usize>>,
}

impl<'a> Frame<'a> {
    fn dfs(&mut self, i: usize, count: u32, dev: u64, best: &mut Option<Best>) {
        if let Some(b) = best {
            if dev + self.min_dev_rest[i] > b.key.deviation {
                return;
            }
        }
        if i == self.names.len() {
            let mut cuts: BTreeMap<String, Vec<usize>> = BTreeMap::new();
            for (j, name) in self.names.iter().enumerate() {
                cuts.insert(name.clone(), self.assigned[j].clone());
            }
            let held_out = predictions(self.problem, self.l, &cuts);
            let map = PlasmidMap { cloning_site: self.c, insert_length: self.l, cuts, held_out };
            let k = key(&map, dev);
            debug_assert_eq!(k.internal, count);
            let better = match best {
                Some(b) => k < b.key,
                None => true,
            };
            if better {
                *best = Some(Best { key: k, map });
            }
            return;
        }
        for choice in 0..self.per[i].len() {
            let cand = self.per[i][choice];
            if let Some(b) = &*best {
                if dev + cand.deviation + self.min_dev_rest[i + 1] > b.key.deviation {
                    // lists are ascending by deviation
                    break;
                }
            }
            let mut positions = self.required[i].clone();
            positions.extend(cand.internal.iter().copied());
            positions.sort_unstable();
            self.assigned[i] = positions;
            let mut extra = 0u64;
            let mut ok = true;
            for (last, d) in self.multi {
                if *last != i {
                    continue;
                }
                let mut union: Vec<usize> = Vec::new();
                for name in &d.enzymes {
                    let j = self.names.iter().position(|x| x == name).expect("panel enzyme");
                    union.extend(self.assigned[j].iter().copied());
                }
                union.sort_unstable();
                union.dedup();
                let visible = digest::visible(&digest::fragments(&union, self.n), self.problem.detection_limit);
                match digest::deviation(&visible, &d.bands, self.problem.error_percent) {
                    Some(x) => extra += x,
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                self.dfs(i + 1, count + cand.internal.len() as u32, dev + cand.deviation + extra, best);
            }
        }
        self.assigned[i].clear();
    }
}

/// Joins one configuration list per enzyme (all of one site count each) at
/// cloning site `c`, over every insert length all lists admit, updating
/// `best`.
pub fn join(problem: &Problem, contexts: &[EnzymeContext], c: usize, lists: &[&HashMap<usize, Vec<Candidate>>], best: &mut Option<Best>) {
    let names: Vec<String> = contexts.iter().map(|x| x.name.clone()).collect();
    if lists.is_empty() {
        return;
    }
    let mut lengths: Vec<usize> = lists[0].keys().copied().collect();
    lengths.retain(|l| lists.iter().all(|m| m.contains_key(l)));
    lengths.sort_unstable();
    let multi: Vec<(usize, &Digest)> = problem
        .observed
        .iter()
        .filter(|d| d.enzymes.len() > 1)
        .map(|d| (d.enzymes.iter().map(|e| names.iter().position(|x| x == e).expect("panel enzyme")).max().unwrap_or(0), d))
        .collect();
    for l in lengths {
        let mut per: Vec<Vec<&Candidate>> = lists.iter().map(|m| m[&l].iter().collect()).collect();
        for list in per.iter_mut() {
            list.sort_by(|x, y| (x.deviation, &x.internal).cmp(&(y.deviation, &y.internal)));
        }
        let mut min_dev_rest = vec![0u64; names.len() + 1];
        for i in (0..names.len()).rev() {
            min_dev_rest[i] = min_dev_rest[i + 1] + per[i].iter().map(|x| x.deviation).min().unwrap_or(0);
        }
        if let Some(b) = &*best {
            if min_dev_rest[0] > b.key.deviation {
                continue;
            }
        }
        let required: Vec<Vec<usize>> = problem.enzymes.iter().map(|e| sites::required_positions(&problem.backbone.seq, e, c, l)).collect();
        let mut frame = Frame {
            problem,
            names: &names,
            required,
            per,
            multi: &multi,
            min_dev_rest,
            c,
            l,
            n: problem.v() + l,
            assigned: vec![Vec::new(); names.len()],
        };
        frame.dfs(0, 0, 0, best);
    }
}
