//! Replay of the recovered programs over every link of every object.

use super::operands::{place, symbol_operands, Unit};
use super::semantics::{replay_pair, replay_single};
use crate::corpus::Corpus;
use crate::program::{Prediction, Program};
use std::collections::BTreeMap;

pub fn predict_all(corpus: &Corpus, units: &[Vec<Unit>], programs: &BTreeMap<u32, Program>) -> Vec<Prediction> {
    let mut out = Vec::new();
    for (o, us) in corpus.objects.iter().zip(units) {
        for link in &o.links {
            for u in us {
                match u {
                    Unit::Single(i) => {
                        let site = &o.sites[*i];
                        let word = match (programs.get(&site.r_type), symbol_operands(o, link, site), place(link, site)) {
                            (Some(p), Some((s, b)), Some(pl)) => replay_single(p, s, 0, pl, b, site.object_word),
                            _ => None,
                        };
                        out.push(Prediction { object: o.id.clone(), link: link.id.clone(), section: site.section.clone(), offset: site.offset, word });
                    }
                    Unit::Pair(i, j) => {
                        let (first, second) = (&o.sites[*i], &o.sites[*j]);
                        let (w1, w2) = match (programs.get(&first.r_type), programs.get(&second.r_type), symbol_operands(o, link, first), place(link, first)) {
                            (Some(pf), Some(ps), Some((s, b)), Some(pl)) => replay_pair(pf, ps, s, first.addend as i128, pl, b, first.object_word, second.object_word),
                            _ => (None, None),
                        };
                        out.push(Prediction { object: o.id.clone(), link: link.id.clone(), section: first.section.clone(), offset: first.offset, word: w1 });
                        out.push(Prediction { object: o.id.clone(), link: link.id.clone(), section: second.section.clone(), offset: second.offset, word: w2 });
                    }
                }
            }
        }
    }
    out
}
