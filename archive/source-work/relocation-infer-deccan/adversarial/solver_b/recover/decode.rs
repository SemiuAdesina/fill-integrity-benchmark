//! Physical-first layout enumeration and decoded-value tables.

use super::{Obs, Record};
use crate::bits::extract;
use crate::program::{Overflow, Signedness, Slice};
use std::collections::BTreeMap;

fn runs_of(mask: u64, container: u32) -> Vec<(u32, u32)> {
    let mut runs = Vec::new();
    let mut j = 0;
    while j < container {
        if mask >> j & 1 == 1 {
            let start = j;
            while j < container && mask >> j & 1 == 1 { j += 1; }
            runs.push((start, j));
        } else {
            j += 1;
        }
    }
    runs
}

fn normal_form(layout: &[Slice]) -> bool {
    layout.windows(2).all(|w| w[0].lsb + w[0].width != w[1].lsb)
}

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 { return vec![items.to_vec()]; }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, head.clone());
            out.push(tail);
        }
    }
    out
}

/// Every normal-form way of cutting the changed-bit runs into at most three
/// ordered slices.
fn cuts(mask: u64, container: u32) -> Vec<Vec<Slice>> {
    let runs = runs_of(mask, container);
    if runs.is_empty() || runs.len() > 3 { return Vec::new(); }
    let mut out = Vec::new();
    // piece counts per run, total <= 3
    let mut counts = vec![1usize; runs.len()];
    loop {
        if counts.iter().sum::<usize>() <= 3 {
            // enumerate cut positions per run
            let mut options: Vec<Vec<Vec<Slice>>> = Vec::new();
            let mut feasible = true;
            for (&(lo, hi), &m) in runs.iter().zip(&counts) {
                let n = (hi - lo) as usize;
                if m > n { feasible = false; break; }
                let mut opts = Vec::new();
                let interior: Vec<u32> = (lo + 1..hi).collect();
                for combo in combinations(&interior, m - 1) {
                    let mut bounds = vec![lo];
                    bounds.extend(combo);
                    bounds.push(hi);
                    opts.push(bounds.windows(2).map(|w| Slice { lsb: w[0], width: w[1] - w[0] }).collect::<Vec<Slice>>());
                }
                options.push(opts);
            }
            if feasible {
                let mut idx = vec![0usize; options.len()];
                loop {
                    let pieces: Vec<Slice> = options.iter().zip(&idx).flat_map(|(o, &i)| o[i].clone()).collect();
                    for perm in permutations(&pieces) {
                        if normal_form(&perm) { out.push(perm); }
                    }
                    let mut k = 0;
                    loop {
                        if k == idx.len() { break; }
                        idx[k] += 1;
                        if idx[k] < options[k].len() { break; }
                        idx[k] = 0;
                        k += 1;
                    }
                    if k == idx.len() { break; }
                }
            }
        }
        // next counts vector
        let mut k = 0;
        loop {
            if k == counts.len() { return out; }
            counts[k] += 1;
            if counts[k] <= 3 { break; }
            counts[k] = 1;
            k += 1;
        }
    }
}

fn combinations(items: &[u32], k: usize) -> Vec<Vec<u32>> {
    if k == 0 { return vec![Vec::new()]; }
    let mut out = Vec::new();
    for (i, &x) in items.iter().enumerate() {
        for mut rest in combinations(&items[i + 1..], k - 1) {
            rest.insert(0, x);
            out.push(rest);
        }
    }
    out
}

/// Decoded-value table of one relocation type: for every candidate cut, the
/// tuple of values decoded from the observed legacy words, indexed by width.
pub struct Table {
    by_width: BTreeMap<u32, BTreeMap<Vec<u64>, Vec<Vec<Slice>>>>,
}

impl Table {
    pub fn new(container: u32, obs: &[Obs]) -> Table {
        let observed: Vec<(u64, u64)> = obs.iter().filter_map(|o| if let Record::Word(w) = o.rec { Some((o.obj, w)) } else { None }).collect();
        let mut mask = 0u64;
        for (obj, w) in &observed { mask |= obj ^ w; }
        let mut by_width: BTreeMap<u32, BTreeMap<Vec<u64>, Vec<Vec<Slice>>>> = BTreeMap::new();
        if !observed.is_empty() {
            for lay in cuts(mask, container) {
                let width: u32 = lay.iter().map(|s| s.width).sum();
                let key: Vec<u64> = observed.iter().map(|(_, w)| extract(*w, &lay)).collect();
                by_width.entry(width).or_default().entry(key).or_default().push(lay);
            }
        }
        Table { by_width }
    }

    /// (signedness, overflow, layout) triples under which the expected
    /// component values reproduce every observed word and every record.
    pub fn matches(&self, values: &[i128], obs: &[Obs]) -> Vec<(Signedness, Overflow, Vec<Slice>)> {
        let mut out = Vec::new();
        for (&width, table) in &self.by_width {
            let key: Vec<u64> = values.iter().zip(obs).filter(|(_, o)| matches!(o.rec, Record::Word(_))).map(|(v, _)| pattern(*v, width)).collect();
            let lays = match table.get(&key) { Some(l) => l, None => continue };
            for sign in [Signedness::Unsigned, Signedness::Signed] {
                for ovf in [Overflow::Reject, Overflow::Wrap] {
                    let consistent = values.iter().zip(obs).all(|(v, o)| {
                        let inr = in_range(*v, width, sign);
                        match o.rec {
                            Record::None => true,
                            Record::Rejected => !inr && ovf == Overflow::Reject,
                            Record::Accepted | Record::Word(_) => inr || ovf == Overflow::Wrap,
                        }
                    });
                    if consistent {
                        for lay in lays { out.push((sign, ovf, lay.clone())); }
                    }
                }
            }
        }
        out
    }
}

pub fn in_range(x: i128, width: u32, sign: Signedness) -> bool {
    match sign {
        Signedness::Signed => x >= -(1i128 << (width - 1)) && x < (1i128 << (width - 1)),
        Signedness::Unsigned => x >= 0 && x < (1i128 << width),
    }
}

pub fn pattern(x: i128, width: u32) -> u64 {
    (x & ((1i128 << width) - 1)) as u64
}
