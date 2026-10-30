//! Hypothesis enumeration over the public grammar.
//!
//! For a scalar hypothesis (expression, scale, signedness, overflow, width) the
//! encoded bit pattern of every observed word is known; a layout must map each
//! logical bit to a container bit carrying exactly that pattern across all
//! observed words, leave every unmapped bit preserved, and be in normal form.
//! Placement is a depth-first walk over logical bits using an index from bit
//! signature to container positions, so a wrong hypothesis dies at the first
//! logical bit no position can carry.

use super::operands::{Observation, Outcome, PairObservation};
use super::semantics::{encode, scaled, split, value};
use crate::program::{Expression, Overflow, Pair, Program, Role, Signedness, Slice, MAX_SLICES, SCALES};
use std::collections::HashMap;

/// Observed words of one member and the physical signature index.
struct Index {
    container: u32,
    words: Vec<(u64, u64)>, // (object word, legacy word)
    preserved: Vec<bool>,
    by_signature: HashMap<Vec<bool>, Vec<u32>>,
}

impl Index {
    fn new(container: u32, words: Vec<(u64, u64)>) -> Index {
        let mut by_signature: HashMap<Vec<bool>, Vec<u32>> = HashMap::new();
        let mut preserved = Vec::with_capacity(container as usize);
        for j in 0..container {
            let sig: Vec<bool> = words.iter().map(|(_, out)| (out >> j) & 1 == 1).collect();
            by_signature.entry(sig).or_default().push(j);
            preserved.push(words.iter().all(|(obj, out)| (obj >> j) & 1 == (out >> j) & 1));
        }
        Index { container, words, preserved, by_signature }
    }

    /// Every normal-form layout of `width` logical bits whose patterns are `bits`.
    fn layouts(&self, bits: &[u64], width: u32) -> Vec<Vec<Slice>> {
        let mut compat: Vec<&Vec<u32>> = Vec::with_capacity(width as usize);
        for t in 0..width {
            let sig: Vec<bool> = bits.iter().map(|b| (b >> t) & 1 == 1).collect();
            match self.by_signature.get(&sig) {
                Some(v) => compat.push(v),
                None => return Vec::new(),
            }
        }
        let mut out = Vec::new();
        let mut slices: Vec<Slice> = Vec::new();
        self.walk(0, width, None, 0, &compat, &mut slices, &mut out);
        out
    }

    fn walk(&self, t: u32, width: u32, prev: Option<u32>, used: u64, compat: &[&Vec<u32>], slices: &mut Vec<Slice>, out: &mut Vec<Vec<Slice>>) {
        if t == width {
            for j in 0..self.container {
                if used >> j & 1 == 0 && !self.preserved[j as usize] { return; }
            }
            out.push(slices.clone());
            return;
        }
        for &j in compat[t as usize] {
            if used >> j & 1 == 1 { continue; }
            if prev == Some(j.wrapping_sub(1)) && j > 0 {
                slices.last_mut().unwrap().width += 1;
                self.walk(t + 1, width, Some(j), used | 1 << j, compat, slices, out);
                slices.last_mut().unwrap().width -= 1;
            } else {
                if slices.len() == MAX_SLICES { continue; }
                slices.push(Slice { lsb: j, width: 1 });
                self.walk(t + 1, width, Some(j), used | 1 << j, compat, slices, out);
                slices.pop();
            }
        }
    }
}

/// Enumerate (signedness, overflow, layout) for one member given its component
/// value in every observation and the recorded outcomes.
fn component_fits(index: &Index, comps: &[(i128, Outcome)]) -> Vec<(Signedness, Overflow, Vec<Slice>)> {
    let mut fits = Vec::new();
    if index.words.is_empty() { return fits; }
    for sign in [Signedness::Unsigned, Signedness::Signed] {
        for ovf in [Overflow::Reject, Overflow::Wrap] {
            'width: for width in 1..=index.container {
                let mut bits = Vec::with_capacity(index.words.len());
                for (x, outcome) in comps {
                    match outcome {
                        Outcome::Unknown => {}
                        Outcome::Rejected => { if encode(*x, 64, sign, ovf).is_some() { continue 'width; } }
                        Outcome::Accepted => { if encode(*x, 64, sign, ovf).is_none() { continue 'width; } }
                        Outcome::Word(_) => match encode(*x, width, sign, ovf) {
                            Some(b) => bits.push(b),
                            None => continue 'width,
                        },
                    }
                }
                for lay in index.layouts(&bits, width) {
                    fits.push((sign, ovf, lay));
                }
            }
        }
    }
    fits
}

fn words_of<'a>(it: impl Iterator<Item = (u64, &'a Outcome)>) -> Vec<(u64, u64)> {
    it.filter_map(|(obj, o)| if let Outcome::Word(w) = o { Some((obj, *w)) } else { None }).collect()
}

pub fn fit_single(r_type: u32, container: u32, obs: &[Observation]) -> Vec<Program> {
    let index = Index::new(container, words_of(obs.iter().map(|o| (o.object_word, &o.outcome))));
    let mut fits = Vec::new();
    for expr in Expression::ALL {
        for scale in SCALES {
            let comps: Vec<(i128, Outcome)> = obs.iter().map(|o| (scaled(value(expr, o.s, o.a, o.p, o.b), scale), o.outcome)).collect();
            for (sign, ovf, slices) in component_fits(&index, &comps) {
                fits.push(Program { r_type, expression: expr, scale, signedness: sign, overflow: ovf, slices, pair: None });
            }
        }
    }
    fits
}

/// Joint fits for a pair group, returned as (program of `lo_id`, program of `hi_id`).
pub fn fit_pair(group: &str, lo_id: u32, c_lo: u32, hi_id: u32, c_hi: u32, obs: &[PairObservation]) -> Vec<(Program, Program)> {
    let idx_a = Index::new(c_lo, words_of(obs.iter().map(|o| (o.lo_type.0, &o.lo_type.1))));
    let idx_b = Index::new(c_hi, words_of(obs.iter().map(|o| (o.hi_type.0, &o.hi_type.1))));
    let mut fits = Vec::new();
    for expr in Expression::ALL {
        for scale in SCALES {
            let qs: Vec<i128> = obs.iter().map(|o| scaled(value(expr, o.s, o.a, o.p, o.b), scale)).collect();
            for k in 1..=63u32 {
                let parts: Vec<(i128, i128)> = qs.iter().map(|q| split(*q, k)).collect();
                for (role_a, role_b) in [(Role::Hi, Role::Lo), (Role::Lo, Role::Hi)] {
                    let pick = |role: Role, (hi, lo): (i128, i128)| if role == Role::Hi { hi } else { lo };
                    let comps_a: Vec<(i128, Outcome)> = parts.iter().zip(obs).map(|(p, o)| (pick(role_a, *p), o.lo_type.1)).collect();
                    let fa = component_fits(&idx_a, &comps_a);
                    if fa.is_empty() { continue; }
                    let comps_b: Vec<(i128, Outcome)> = parts.iter().zip(obs).map(|(p, o)| (pick(role_b, *p), o.hi_type.1)).collect();
                    let fb = component_fits(&idx_b, &comps_b);
                    for (sa, oa, la) in &fa {
                        for (sb, ob, lb) in &fb {
                            fits.push((
                                Program { r_type: lo_id, expression: expr, scale, signedness: *sa, overflow: *oa, slices: la.clone(), pair: Some(Pair { group: group.to_string(), role: role_a, split: k }) },
                                Program { r_type: hi_id, expression: expr, scale, signedness: *sb, overflow: *ob, slices: lb.clone(), pair: Some(Pair { group: group.to_string(), role: role_b, split: k }) },
                            ));
                        }
                    }
                }
            }
        }
    }
    fits
}
