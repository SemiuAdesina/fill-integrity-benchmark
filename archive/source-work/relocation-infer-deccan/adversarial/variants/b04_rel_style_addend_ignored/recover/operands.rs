//! Operand resolution and observation collection (spec: elf-corpus-v1.md §4–§6).

use crate::corpus::{Corpus, Link, LinkResult, Object, Site};
use crate::elf::{SHN_ABS, SHN_UNDEF};

/// What the legacy linker recorded for one site in one link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The link succeeded; this is the patched word.
    Word(u64),
    /// The link failed and this site was among the rejected ones.
    Rejected,
    /// The link failed because of other sites; this one was accepted.
    Accepted,
    /// A replay link: nothing recorded.
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Observation {
    pub s: i128,
    pub a: i128,
    pub p: i128,
    pub b: i128,
    pub object_word: u64,
    pub outcome: Outcome,
}

/// A relocation unit of an object: one ordinary site or one pair instance
/// (first record, second record) in entry order.
#[derive(Clone, Debug)]
pub enum Unit {
    Single(usize),
    Pair(usize, usize),
}

/// Group the sites of every object into units, in section/offset order.
pub fn units(corpus: &Corpus) -> Result<Vec<Vec<Unit>>, String> {
    let mut all = Vec::new();
    for o in &corpus.objects {
        let mut v = Vec::new();
        let mut i = 0;
        while i < o.sites.len() {
            let s = &o.sites[i];
            let group = corpus.reloc_type(s.r_type).and_then(|t| t.pair_group.clone());
            if let Some(g) = group {
                let partner = o.sites.get(i + 1).filter(|n| {
                    corpus.reloc_type(n.r_type).and_then(|t| t.pair_group.clone()).as_deref() == Some(g.as_str())
                        && n.r_type != s.r_type && n.symbol == s.symbol && n.addend == s.addend && n.section == s.section
                });
                match partner {
                    Some(_) => { v.push(Unit::Pair(i, i + 1)); i += 2; }
                    None => return Err(format!("{}: pair-group site {}+{} has no partner", o.id, s.section, s.offset)),
                }
            } else {
                v.push(Unit::Single(i));
                i += 1;
            }
        }
        all.push(v);
    }
    Ok(all)
}

/// (S, B) of a site in a link.
pub fn symbol_operands(o: &Object, link: &Link, site: &Site) -> Option<(i128, i128)> {
    let sym = o.elf.symbols.get(site.symbol)?;
    if sym.shndx == SHN_ABS {
        return Some((sym.value as i128, 0));
    }
    if sym.shndx == SHN_UNDEF {
        let r = link.symbols.get(&sym.name)?;
        let b = match &r.section { None => 0, Some(sec) => *link.bases.get(sec)? as i128 };
        return Some((r.value as i128, b));
    }
    let sec = o.elf.sections.get(sym.shndx as usize)?;
    Some((sym.value as i128, *link.bases.get(&sec.name)? as i128))
}

pub fn place(link: &Link, site: &Site) -> Option<i128> {
    Some(*link.bases.get(&site.section)? as i128 + site.offset as i128)
}

pub fn outcome(link: &Link, site: &Site) -> Outcome {
    match &link.result {
        LinkResult::Linked(_) => match link.legacy_word(&site.section, site.offset, site.container_bits) {
            Some(w) => Outcome::Word(w),
            None => Outcome::Unknown,
        },
        LinkResult::Failed { .. } => if link.rejected(&site.section, site.offset) == Some(true) { Outcome::Rejected } else { Outcome::Accepted },
        LinkResult::Replay => Outcome::Unknown,
    }
}

/// Every observation of an ordinary type, over every object and link.
pub fn single_observations(corpus: &Corpus, units: &[Vec<Unit>], r_type: u32) -> Vec<Observation> {
    let mut out = Vec::new();
    for (o, us) in corpus.objects.iter().zip(units) {
        for u in us {
            if let Unit::Single(i) = u {
                let site = &o.sites[*i];
                if site.r_type != r_type { continue; }
                for link in &o.links {
                    if let (Some((s, b)), Some(p)) = (symbol_operands(o, link, site), place(link, site)) {
                        out.push(Observation { s, a: 0, p, b, object_word: site.object_word, outcome: outcome(link, site) });
                    }
                }
            }
        }
    }
    out
}

/// One pair instance in one link: shared operands and both members' records,
/// ordered (lower type id, higher type id).
#[derive(Clone, Debug)]
pub struct PairObservation {
    pub s: i128,
    pub a: i128,
    pub p: i128,
    pub b: i128,
    pub lo_type: (u64, Outcome),
    pub hi_type: (u64, Outcome),
}

pub fn pair_observations(corpus: &Corpus, units: &[Vec<Unit>], lo_id: u32, hi_id: u32) -> Vec<PairObservation> {
    let mut out = Vec::new();
    for (o, us) in corpus.objects.iter().zip(units) {
        for u in us {
            if let Unit::Pair(i, j) = u {
                let (first, second) = (&o.sites[*i], &o.sites[*j]);
                if !((first.r_type == lo_id && second.r_type == hi_id) || (first.r_type == hi_id && second.r_type == lo_id)) { continue; }
                for link in &o.links {
                    if let (Some((s, b)), Some(p)) = (symbol_operands(o, link, first), place(link, first)) {
                        let (lo_site, hi_site) = if first.r_type == lo_id { (first, second) } else { (second, first) };
                        out.push(PairObservation {
                            s, a: first.addend as i128, p, b,
                            lo_type: (lo_site.object_word, outcome(link, lo_site)),
                            hi_type: (hi_site.object_word, outcome(link, hi_site)),
                        });
                    }
                }
            }
        }
    }
    out
}
