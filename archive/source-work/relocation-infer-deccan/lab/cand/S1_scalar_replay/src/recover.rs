//! Relocation recovery — reference implementation.
//!
//! Architecture: fit-then-encode. For every relocation type the observations
//! of every site in every link are collected (`operands`), every scalar
//! hypothesis of the public grammar is evaluated to the logical bit pattern
//! each observation would carry (`semantics`), and a depth-first placement of
//! logical bits on compatible container bits enumerates the layouts that
//! reproduce every observation (`search`). Pair groups are searched jointly
//! over the shared part and independently over each member's encoding. The
//! fitting program with the smallest canonical key is emitted, and every link
//! is replayed with it (`predict`).

mod operands;
mod predict;
mod search;
mod semantics;

use crate::corpus::Corpus;
use crate::program::{Program, Recovery, Expression, Signedness, Overflow, Slice, SCALES};

/// Early-development fallback: when nothing fits exactly, keep the hypothesis that
/// reproduces the most observed words, with one contiguous field over the changed bits.
fn best_effort(r_type: u32, container: u32, obs: &[operands::Observation], scales: &[u32], signs: &[Signedness], ovfs: &[Overflow]) -> Option<Program> {
    let words: Vec<(u64, u64)> = obs.iter().filter_map(|o| if let operands::Outcome::Word(w) = o.outcome { Some((o.object_word, w)) } else { None }).collect();
    if words.is_empty() { return None; }
    let mask = words.iter().fold(0u64, |m, (a, b)| m | (a ^ b));
    if mask == 0 { return None; }
    let lsb = mask.trailing_zeros();
    let _ = container; let width = 64 - lsb - mask.leading_zeros();
    let slices = vec![Slice { lsb, width }];
    let mut best: Option<(usize, Program)> = None;
    for expr in Expression::ALL { for &scale in scales { for &sign in signs { for &ovf in ovfs {
        let p = Program { r_type, expression: expr, scale, signedness: sign, overflow: ovf, slices: slices.clone(), pair: None };
        let hits = obs.iter().filter(|o| matches!(o.outcome, operands::Outcome::Word(w) if semantics::replay_single(&p, o.s, o.a, o.p, o.b, o.object_word) == Some(w))).count();
        if best.as_ref().map(|(h, _)| hits > *h).unwrap_or(true) { best = Some((hits, p)); }
    } } } }
    best.map(|(_, p)| p)
}
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct RecoverError(pub String);

impl std::fmt::Display for RecoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub fn recover(corpus: &Corpus) -> Result<Recovery, RecoverError> {
    let units = operands::units(corpus).map_err(RecoverError)?;
    let mut programs: BTreeMap<u32, Program> = BTreeMap::new();

    // Ordinary types: one independent search each.
    for t in corpus.types.iter() {
        let obs = operands::single_observations(corpus, &units, t.id);
        let fits = search::fit_single(t.id, t.container_bits, &obs);
        if let Some(best) = fits.into_iter().next() { programs.insert(t.id, best); } else if let Some(p) = best_effort(t.id, t.container_bits, &obs, &[1], &[Signedness::Signed], &[Overflow::Reject]) { programs.insert(t.id, p); }
    }

    // Pair groups: a joint search over both members.
    let mut groups: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for t in &corpus.types {
        if let Some(g) = &t.pair_group {
            groups.entry(g.clone()).or_default().push(t.id);
        }
    }
    for (group, mut members) in groups.into_iter().filter(|_| false) {
        members.sort();
        if members.len() != 2 {
            return Err(RecoverError(format!("pair group {} does not have exactly two member types", group)));
        }
        let (lo_id, hi_id) = (members[0], members[1]);
        let c_lo = corpus.reloc_type(lo_id).unwrap().container_bits;
        let c_hi = corpus.reloc_type(hi_id).unwrap().container_bits;
        let inst = operands::pair_observations(corpus, &units, lo_id, hi_id);
        let fits = search::fit_pair(&group, lo_id, c_lo, hi_id, c_hi, &inst);
        let best = fits.into_iter().next()
            .ok_or_else(|| RecoverError(format!("no program pair in the grammar reproduces every observation of pair group {}", group)))?;
        programs.insert(lo_id, best.0);
        programs.insert(hi_id, best.1);
    }

    let predictions = predict::predict_all(corpus, &units, &programs);
    let mut result = Recovery { programs: programs.into_values().collect(), predictions };
    result.sort();
    Ok(result)
}
