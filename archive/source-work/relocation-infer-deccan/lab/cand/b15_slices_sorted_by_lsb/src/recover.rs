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
use crate::program::{Program, Recovery};
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
    for t in corpus.types.iter().filter(|t| t.pair_group.is_none()) {
        let obs = operands::single_observations(corpus, &units, t.id);
        let fits = search::fit_single(t.id, t.container_bits, &obs);
        let best = fits.into_iter().min_by_key(|p| p.key())
            .ok_or_else(|| RecoverError(format!("no program in the grammar reproduces every observation of type {}", t.id)))?;
        programs.insert(t.id, best);
    }

    // Pair groups: a joint search over both members.
    let mut groups: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for t in &corpus.types {
        if let Some(g) = &t.pair_group {
            groups.entry(g.clone()).or_default().push(t.id);
        }
    }
    for (group, mut members) in groups {
        members.sort();
        if members.len() != 2 {
            return Err(RecoverError(format!("pair group {} does not have exactly two member types", group)));
        }
        let (lo_id, hi_id) = (members[0], members[1]);
        let c_lo = corpus.reloc_type(lo_id).unwrap().container_bits;
        let c_hi = corpus.reloc_type(hi_id).unwrap().container_bits;
        let inst = operands::pair_observations(corpus, &units, lo_id, hi_id);
        let fits = search::fit_pair(&group, lo_id, c_lo, hi_id, c_hi, &inst);
        let best = fits.into_iter().min_by_key(|(a, b)| (a.key(), b.key()))
            .ok_or_else(|| RecoverError(format!("no program pair in the grammar reproduces every observation of pair group {}", group)))?;
        programs.insert(lo_id, best.0);
        programs.insert(hi_id, best.1);
    }

    for p in programs.values_mut() { p.slices.sort_by_key(|s| s.lsb); }
    let predictions = predict::predict_all(corpus, &units, &programs);
    let mut result = Recovery { programs: programs.into_values().collect(), predictions };
    result.sort();
    Ok(result)
}
