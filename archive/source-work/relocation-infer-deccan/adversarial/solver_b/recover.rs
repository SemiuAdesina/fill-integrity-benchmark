//! Relocation recovery — independent architecture B (physical-first).
//!
//! The legacy words are read before any hypothesis is formed: for every type
//! the set of container bits the legacy linker ever changed is computed, all
//! normal-form cuts of that set into at most three ordered slices are
//! enumerated from the physical side, and the logical value of every observed
//! word is DECODED under each cut. Scalar semantics are then fitted by direct
//! equation checking against the decoded values, pairs are solved as a join
//! over the two members' decoded tables, and canonicalisation happens last
//! over the complete fit set. No logical-bit placement search is performed.

mod decode;
mod replay;

use crate::corpus::{Corpus, Link, LinkResult, Object, Site};
use crate::elf::{SHN_ABS, SHN_UNDEF};
use crate::program::{Expression, Overflow, Pair, Prediction, Program, Recovery, Role, Signedness, SCALES};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct RecoverError(pub String);

impl std::fmt::Display for RecoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}

/// Legacy record for one site in one link.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Record { Word(u64), Rejected, Accepted, None }

#[derive(Clone, Debug)]
pub struct Obs { pub s: i128, pub a: i128, pub p: i128, pub b: i128, pub obj: u64, pub rec: Record }

pub fn resolve(o: &Object, l: &Link, site: &Site) -> Option<(i128, i128, i128)> {
    let sym = o.elf.symbols.get(site.symbol)?;
    let p = *l.bases.get(&site.section)? as i128 + site.offset as i128;
    if sym.shndx == SHN_ABS { return Some((sym.value as i128, p, 0)); }
    if sym.shndx == SHN_UNDEF {
        let r = l.symbols.get(&sym.name)?;
        let b = match &r.section { None => 0, Some(s) => *l.bases.get(s)? as i128 };
        return Some((r.value as i128, p, b));
    }
    let sec = o.elf.sections.get(sym.shndx as usize)?;
    Some((sym.value as i128, p, *l.bases.get(&sec.name)? as i128))
}

pub fn record(l: &Link, site: &Site) -> Record {
    match &l.result {
        LinkResult::Linked(_) => l.legacy_word(&site.section, site.offset, site.container_bits).map(Record::Word).unwrap_or(Record::None),
        LinkResult::Failed { .. } => if l.rejected(&site.section, site.offset) == Some(true) { Record::Rejected } else { Record::Accepted },
        LinkResult::Replay => Record::None,
    }
}

/// Site groups per object: single index or (first, second) of a pair instance.
pub fn groups(corpus: &Corpus, o: &Object) -> Result<Vec<(usize, Option<usize>)>, String> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < o.sites.len() {
        let s = &o.sites[i];
        let g = corpus.reloc_type(s.r_type).and_then(|t| t.pair_group.clone());
        if let Some(g) = g {
            let n = o.sites.get(i + 1).ok_or_else(|| format!("{}: dangling pair member", o.id))?;
            let ng = corpus.reloc_type(n.r_type).and_then(|t| t.pair_group.clone());
            if ng.as_deref() != Some(g.as_str()) || n.r_type == s.r_type || n.symbol != s.symbol || n.addend != s.addend || n.section != s.section {
                return Err(format!("{}: site {}+{} has no pair partner", o.id, s.section, s.offset));
            }
            v.push((i, Some(i + 1)));
            i += 2;
        } else {
            v.push((i, None));
            i += 1;
        }
    }
    Ok(v)
}

pub fn recover(corpus: &Corpus) -> Result<Recovery, RecoverError> {
    let mut per_object = Vec::new();
    for o in &corpus.objects { per_object.push(groups(corpus, o).map_err(RecoverError)?); }

    // observations per type (single) and per pair group (instances keyed by lower type id)
    let mut single: BTreeMap<u32, Vec<Obs>> = BTreeMap::new();
    let mut pairs: BTreeMap<String, Vec<(Obs, Obs)>> = BTreeMap::new(); // (lower-id member obs, higher-id member obs)
    for (o, gs) in corpus.objects.iter().zip(&per_object) {
        for l in &o.links {
            for &(i, j) in gs {
                match j {
                    None => {
                        let site = &o.sites[i];
                        if let Some((s, p, b)) = resolve(o, l, site) {
                            single.entry(site.r_type).or_default().push(Obs { s, a: site.addend as i128, p, b, obj: site.object_word, rec: record(l, site) });
                        }
                    }
                    Some(j) => {
                        let (first, second) = (&o.sites[i], &o.sites[j]);
                        let g = corpus.reloc_type(first.r_type).unwrap().pair_group.clone().unwrap();
                        if let Some((s, p, b)) = resolve(o, l, first) {
                            let mk = |site: &Site| Obs { s, a: first.addend as i128, p, b, obj: site.object_word, rec: record(l, site) };
                            let (lo, hi) = if first.r_type < second.r_type { (mk(first), mk(second)) } else { (mk(second), mk(first)) };
                            pairs.entry(g).or_default().push((lo, hi));
                        }
                    }
                }
            }
        }
    }

    let mut programs: BTreeMap<u32, Program> = BTreeMap::new();
    for t in corpus.types.iter().filter(|t| t.pair_group.is_none()) {
        let obs = single.remove(&t.id).unwrap_or_default();
        let table = decode::Table::new(t.container_bits, &obs);
        let mut fits: Vec<Program> = Vec::new();
        for expr in Expression::ALL {
            for scale in SCALES {
                let vals: Vec<i128> = obs.iter().map(|o| replay::scaled(replay::eval(expr, o.s, o.a, o.p, o.b), scale)).collect();
                for (sign, ovf, slices) in table.matches(&vals, &obs) {
                    fits.push(Program { r_type: t.id, expression: expr, scale, signedness: sign, overflow: ovf, slices, pair: None });
                }
            }
        }
        let best = fits.into_iter().min_by_key(|p| p.key()).ok_or_else(|| RecoverError(format!("type {}: no program reproduces the corpus", t.id)))?;
        programs.insert(t.id, best);
    }
    for t in corpus.types.iter().filter(|t| t.pair_group.is_some()) {
        if programs.contains_key(&t.id) { continue; }
        let g = t.pair_group.clone().unwrap();
        let mut ids: Vec<u32> = corpus.types.iter().filter(|u| u.pair_group.as_deref() == Some(g.as_str())).map(|u| u.id).collect();
        ids.sort();
        let (lo_id, hi_id) = (ids[0], ids[1]);
        let inst = pairs.remove(&g).unwrap_or_default();
        let obs_lo: Vec<Obs> = inst.iter().map(|(a, _)| a.clone()).collect();
        let obs_hi: Vec<Obs> = inst.iter().map(|(_, b)| b.clone()).collect();
        let tab_lo = decode::Table::new(corpus.reloc_type(lo_id).unwrap().container_bits, &obs_lo);
        let tab_hi = decode::Table::new(corpus.reloc_type(hi_id).unwrap().container_bits, &obs_hi);
        let mut fits: Vec<(Program, Program)> = Vec::new();
        for expr in Expression::ALL {
            for scale in SCALES {
                let qs: Vec<i128> = obs_lo.iter().map(|o| replay::scaled(replay::eval(expr, o.s, o.a, o.p, o.b), scale)).collect();
                for k in 1..=63u32 {
                    let parts: Vec<(i128, i128)> = qs.iter().map(|q| replay::split(*q, k)).collect();
                    for (ra, rb) in [(Role::Hi, Role::Lo), (Role::Lo, Role::Hi)] {
                        let va: Vec<i128> = parts.iter().map(|(h, l)| if ra == Role::Hi { *h } else { *l }).collect();
                        let ma = tab_lo.matches(&va, &obs_lo);
                        if ma.is_empty() { continue; }
                        let vb: Vec<i128> = parts.iter().map(|(h, l)| if rb == Role::Hi { *h } else { *l }).collect();
                        let mb = tab_hi.matches(&vb, &obs_hi);
                        for (sa, oa, la) in &ma {
                            for (sb, ob, lb) in &mb {
                                fits.push((
                                    Program { r_type: lo_id, expression: expr, scale, signedness: *sa, overflow: *oa, slices: la.clone(), pair: Some(Pair { group: g.clone(), role: ra, split: k }) },
                                    Program { r_type: hi_id, expression: expr, scale, signedness: *sb, overflow: *ob, slices: lb.clone(), pair: Some(Pair { group: g.clone(), role: rb, split: k }) },
                                ));
                            }
                        }
                    }
                }
            }
        }
        let best = fits.into_iter().min_by_key(|(a, b)| (a.key(), b.key())).ok_or_else(|| RecoverError(format!("pair group {}: no program pair reproduces the corpus", g)))?;
        programs.insert(lo_id, best.0);
        programs.insert(hi_id, best.1);
    }

    let mut predictions = Vec::new();
    for (o, gs) in corpus.objects.iter().zip(&per_object) {
        for l in &o.links {
            for &(i, j) in gs {
                match j {
                    None => {
                        let site = &o.sites[i];
                        let word = resolve(o, l, site).and_then(|(s, p, b)| replay::single(&programs[&site.r_type], s, site.addend as i128, p, b, site.object_word));
                        predictions.push(Prediction { object: o.id.clone(), link: l.id.clone(), section: site.section.clone(), offset: site.offset, word });
                    }
                    Some(j) => {
                        let (first, second) = (&o.sites[i], &o.sites[j]);
                        let (w1, w2) = match resolve(o, l, first) {
                            Some((s, p, b)) => replay::pair(&programs[&first.r_type], &programs[&second.r_type], s, first.addend as i128, p, b, first.object_word, second.object_word),
                            None => (None, None),
                        };
                        predictions.push(Prediction { object: o.id.clone(), link: l.id.clone(), section: first.section.clone(), offset: first.offset, word: w1 });
                        predictions.push(Prediction { object: o.id.clone(), link: l.id.clone(), section: second.section.clone(), offset: second.offset, word: w2 });
                    }
                }
            }
        }
    }
    let mut result = Recovery { programs: programs.into_values().collect(), predictions };
    result.sort();
    Ok(result)
}

// silence unused warnings for enum variants referenced only through matches
#[allow(dead_code)]
fn _unused(_: Signedness, _: Overflow) {}
