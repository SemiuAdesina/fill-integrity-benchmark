
//! Per-enzyme insert-region configurations consistent with the enzyme's
//! single digest, indexed by the insert length they imply.

use std::collections::HashMap;

use crate::digest;
use crate::input::{Enzyme, Problem};
use crate::sites::{self, Site};

/// What the search needs to know about one enzyme, computed once.
pub struct EnzymeContext {
    pub name: String,
    pub window: usize,
    pub sites: Vec<Site>,
    /// The observed single digest, expanded by multiplicity, ascending.
    pub observed: Vec<u32>,
}

impl EnzymeContext {
    pub fn new(problem: &Problem, enzyme: &Enzyme) -> Self {
        let single = problem.single(&enzyme.name).expect("the input format guarantees a single digest per enzyme");
        EnzymeContext {
            name: enzyme.name.clone(),
            window: enzyme.site.len(),
            sites: sites::scan(&problem.backbone.seq, &enzyme.site, enzyme.cut_offset),
            observed: digest::expand(&single.bands),
        }
    }
}

/// One insert-region configuration: ascending construct positions inside
/// `[c, c + L]`, with the single digest's deviation.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub internal: Vec<usize>,
    pub deviation: u64,
}

/// A slot of a piece sequence: paired with an observed band, or below the
/// detection limit.
#[derive(Clone, Copy)]
enum Slot {
    Band(usize),
    Sub,
}

struct Enumeration<'a> {
    problem: &'a Problem,
    c: usize,
    /// Surviving backbone positions below `c` and at or above `c` (backbone coordinates).
    a: Vec<usize>,
    b: Vec<usize>,
    /// Length of the insert-containing gap minus the insert length.
    g_vec: usize,
    /// `q = base + offset` for insert-region cuts; may be "negative" (see `at`).
    base: i64,
    observed: Vec<u32>,
    out: HashMap<usize, HashMap<Vec<usize>, u64>>,
}

impl<'a> Enumeration<'a> {
    /// Insert length implied when the last piece reaches the first moving site.
    fn insert_length(&self, last_cut: i64, last_piece: usize) -> Option<usize> {
        let v = self.problem.v() as i64;
        let l = if let Some(&first_b) = self.b.first() {
            last_piece as i64 + last_cut - first_b as i64
        } else {
            last_piece as i64 + last_cut - self.a[0] as i64 - v
        };
        if l < self.problem.insert_min as i64 || l > self.problem.insert_max as i64 {
            return None;
        }
        Some(l as usize)
    }

    fn verify(&mut self, cuts: &[i64], l: usize) {
        let c = self.c;
        if cuts.iter().any(|&q| q < c as i64 || q > (c + l) as i64) {
            return;
        }
        let n = self.problem.v() + l;
        let mut positions: Vec<usize> = self.a.clone();
        positions.extend(self.b.iter().map(|&p| p + l));
        positions.extend(cuts.iter().map(|&q| q as usize));
        positions.sort_unstable();
        let before = positions.len();
        positions.dedup();
        if positions.len() != before {
            return;
        }
        let visible = digest::visible(&digest::fragments(&positions, n), self.problem.detection_limit);
        let Some(dev) = digest::deviation(&visible, &group_of(&self.observed), self.problem.error_percent) else {
            return;
        };
        let internal: Vec<usize> = cuts.iter().map(|&q| q as usize).collect();
        self.out.entry(l).or_default().entry(internal).or_insert(dev);
    }

    /// Enumerates the pieces of one slot sequence; `windows[i]` is the
    /// inclusive length range of piece `i`.
    fn pieces(&mut self, windows: &[(usize, usize)], index: usize, cuts: &mut Vec<i64>, sum: usize, rest_min: &[usize], rest_max: &[usize]) {
        let t = windows.len() - 1;
        let (lo, hi) = windows[index];
        let c = self.c as i64;
        let lmin = self.problem.insert_min;
        let lmax = self.problem.insert_max;
        let g_vec = self.g_vec;
        for piece in lo..=hi {
            let total = sum + piece;
            if index == t {
                // The last piece fixes the insert length.
                let last_cut = if t == 0 { 0 } else { cuts[t - 1] };
                let anchor_cut = if t == 0 { self.base } else { last_cut };
                let _ = anchor_cut;
                let l_implied = total as i64 - g_vec as i64;
                if l_implied < lmin as i64 || l_implied > lmax as i64 {
                    continue;
                }
                let l = l_implied as usize;
                if t > 0 {
                    // consistency: the last piece reaches the first moving site
                    if self.insert_length(last_cut, piece) != Some(l) {
                        continue;
                    }
                }
                let cuts_copy = cuts.clone();
                self.verify(&cuts_copy, l);
                continue;
            }
            // Remaining pieces must still allow an insert length within bounds.
            let min_total = total + rest_min[index + 1];
            let max_total = total + rest_max[index + 1];
            if (min_total as i64 - g_vec as i64) > lmax as i64 || (max_total as i64 - g_vec as i64) < lmin as i64 {
                continue;
            }
            let q = self.base + total as i64;
            if q < c || q > c + lmax as i64 {
                if q > c + lmax as i64 {
                    break;
                }
                continue;
            }
            cuts.push(q);
            self.pieces(windows, index + 1, cuts, total, rest_min, rest_max);
            cuts.pop();
        }
    }
}

fn group_of(observed: &[u32]) -> Vec<crate::input::Band> {
    digest::group(observed)
}

/// Every configuration of exactly `t` insert-region cuts of `ctx` at cloning
/// site `c` consistent with its single digest, keyed by the insert length it
/// implies.
pub fn candidates_at(problem: &Problem, ctx: &EnzymeContext, c: usize, t: usize) -> HashMap<usize, Vec<Candidate>> {
    let v = problem.v();
    let d = problem.detection_limit;
    let e = problem.error_percent;
    let surviving: Vec<usize> = {
        let mut s: Vec<usize> = ctx.sites.iter().filter(|s| !sites::spans_junction(s.start, ctx.window, c, v)).map(|s| s.cut).collect();
        s.sort_unstable();
        s.dedup();
        s
    };
    let a: Vec<usize> = surviving.iter().copied().filter(|&p| p < c).collect();
    let b: Vec<usize> = surviving.iter().copied().filter(|&p| p >= c).collect();
    assert!(!a.is_empty() || !b.is_empty(), "the input format guarantees an anchored enzyme");
    let mut fixed: Vec<usize> = a.windows(2).map(|w| w[1] - w[0]).chain(b.windows(2).map(|w| w[1] - w[0])).collect();
    if !a.is_empty() && !b.is_empty() {
        fixed.push(v - b[b.len() - 1] + a[0]);
    }
    let fixed_visible = digest::visible(&fixed, d);
    let observed = ctx.observed.clone();
    if fixed_visible.len() > observed.len() {
        return HashMap::new();
    }
    let g_vec = if !a.is_empty() && !b.is_empty() {
        b[0] - a[a.len() - 1]
    } else if !a.is_empty() {
        v - a[a.len() - 1] + a[0]
    } else {
        v - b[b.len() - 1] + b[0]
    };
    let base: i64 = if let Some(&last_a) = a.last() { last_a as i64 } else { b[b.len() - 1] as i64 - v as i64 };
    let mut en = Enumeration { problem, c, a, b, g_vec, base, observed: observed.clone(), out: HashMap::new() };

    let m = observed.len();
    let visible_needed = m as i64 - fixed_visible.len() as i64;
    {
        // slot sequences of length t + 1 over m bands plus the sub-detection slot
        let choices = m + 1;
        let mut seq = vec![0usize; t + 1];
        loop {
            let slots: Vec<Slot> = seq.iter().map(|&s| if s < m { Slot::Band(s) } else { Slot::Sub }).collect();
            let visible: Vec<u32> = slots.iter().filter_map(|s| if let Slot::Band(i) = s { Some(observed[*i]) } else { None }).collect();
            let plausible = visible.len() as i64 == visible_needed && (d > 1 || visible.len() == t + 1) && {
                let mut pred: Vec<u32> = visible.iter().copied().chain(fixed_visible.iter().copied()).collect();
                pred.sort_unstable();
                pred.iter().zip(observed.iter()).all(|(&p, &o)| {
                    let gap = if p > o { p - o } else { o - p };
                    gap <= digest::tolerance(o, e)
                })
            };
            if plausible {
                let windows: Vec<(usize, usize)> = slots
                    .iter()
                    .map(|s| match s {
                        Slot::Band(i) => {
                            let o = observed[*i];
                            let tol = digest::tolerance(o, e);
                            ((o.saturating_sub(tol)).max(1) as usize, (o + tol) as usize)
                        }
                        Slot::Sub => (1, (d - 1) as usize),
                    })
                    .collect();
                let mut rest_min = vec![0usize; t + 2];
                let mut rest_max = vec![0usize; t + 2];
                for i in (0..=t).rev() {
                    rest_min[i] = rest_min[i + 1] + windows[i].0;
                    rest_max[i] = rest_max[i + 1] + windows[i].1;
                }
                let lo = rest_min[0] as i64 - g_vec as i64;
                let hi = rest_max[0] as i64 - g_vec as i64;
                if hi >= problem.insert_min as i64 && lo <= problem.insert_max as i64 {
                    let mut cuts = Vec::with_capacity(t);
                    en.pieces(&windows, 0, &mut cuts, 0, &rest_min, &rest_max);
                }
            }
            // next sequence
            let mut i = 0;
            loop {
                if i > t {
                    break;
                }
                seq[i] += 1;
                if seq[i] < choices {
                    break;
                }
                seq[i] = 0;
                i += 1;
            }
            if i > t {
                break;
            }
        }
    }
    en.out
        .into_iter()
        .map(|(l, sets)| (l, sets.into_iter().map(|(internal, deviation)| Candidate { internal, deviation }).collect()))
        .collect()
}
