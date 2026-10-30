#!/usr/bin/env python3
"""Generate the adversarial population (slots 03-08) and the partial-fix battery
as candidate source trees derived from the reference by one targeted change
each. Every change is a plausible implementation error, not a sabotage.

Output: adversarial/variants/<name>/recover.rs (+ recover/).
"""
import os, shutil, sys
HERE = os.path.dirname(os.path.abspath(__file__))
REF = os.path.join(HERE, "..", "pkg", "solution", "fixed", "src")
OUT = os.path.join(HERE, "variants")

# name -> (slot/family, description, [(file, old, new), ...])
VARIANTS = {}

def v(name, family, desc, patches):
    VARIANTS[name] = (family, desc, patches)

SEM = "recover/semantics.rs"
OPS = "recover/operands.rs"
SEARCH = "recover/search.rs"
TOP = "recover.rs"
PRED = "recover/predict.rs"

# ---------------------------------------------------------------- population
v("p03_local_fit_first_object", "03 per-site/local fit",
  "fits each relocation type on the first object that carries it and reconciles nothing across objects",
  [(OPS, "pub fn single_observations(corpus: &Corpus, units: &[Vec<Unit>], r_type: u32) -> Vec<Observation> {\n    let mut out = Vec::new();\n    for (o, us) in corpus.objects.iter().zip(units) {",
        "pub fn single_observations(corpus: &Corpus, units: &[Vec<Unit>], r_type: u32) -> Vec<Observation> {\n    let mut out = Vec::new();\n    for (o, us) in corpus.objects.iter().zip(units) {\n        if !out.is_empty() { break; }"),
   (OPS, "pub fn pair_observations(corpus: &Corpus, units: &[Vec<Unit>], lo_id: u32, hi_id: u32) -> Vec<PairObservation> {\n    let mut out = Vec::new();\n    for (o, us) in corpus.objects.iter().zip(units) {",
        "pub fn pair_observations(corpus: &Corpus, units: &[Vec<Unit>], lo_id: u32, hi_id: u32) -> Vec<PairObservation> {\n    let mut out = Vec::new();\n    for (o, us) in corpus.objects.iter().zip(units) {\n        if !out.is_empty() { break; }")])
v("p04_contiguous_field_only", "04 field-naive",
  "assumes every relocation field is one contiguous run of bits",
  [(SEARCH, "                if slices.len() == MAX_SLICES { continue; }", "                if slices.len() == 1 { continue; }")])
v("p05_floor_division", "05 negative arithmetic",
  "divides by the scale with floor semantics instead of truncation toward zero",
  [(SEM, "pub fn scaled(v: i128, scale: u32) -> i128 {\n    v / scale as i128\n}", "pub fn scaled(v: i128, scale: u32) -> i128 {\n    v.div_euclid(scale as i128)\n}")])
v("p06_adjusted_split", "06 pair-naive",
  "splits the shared value RISC-V style: signed low half and a rounded high half",
  [(SEM, "    let m = 1i128 << k;\n    let lo = q.rem_euclid(m);\n    ((q - lo) / m, lo)",
        "    let m = 1i128 << k;\n    let hi = (q + (m >> 1)).div_euclid(m);\n    (hi, q - hi * m)")])
v("p07_prefers_signed", "07 canonicalisation-naive",
  "breaks ties by preferring signed programs, not by the published key",
  [(TOP, "        let best = fits.into_iter().min_by_key(|p| p.key())\n            .ok_or_else(|| RecoverError(format!(\"no program in the grammar reproduces every observation of type {}\", t.id)))?;",
        "        let best = fits.into_iter().min_by_key(|p| { let mut k = p.key(); k.signedness = 1 - k.signedness; k })\n            .ok_or_else(|| RecoverError(format!(\"no program in the grammar reproduces every observation of type {}\", t.id)))?;"),
   (TOP, "        let best = fits.into_iter().min_by_key(|(a, b)| (a.key(), b.key()))",
        "        let best = fits.into_iter().min_by_key(|(a, b)| { let (mut ka, mut kb) = (a.key(), b.key()); ka.signedness = 1 - ka.signedness; kb.signedness = 1 - kb.signedness; (ka, kb) })")])
v("p08_site_section_base", "08 address-model-naive",
  "takes B from the section containing the site instead of the symbol's section",
  [(OPS, "    let sec = o.elf.sections.get(sym.shndx as usize)?;\n    Some((sym.value as i128, *link.bases.get(&sec.name)? as i128))",
        "    let _ = o.elf.sections.get(sym.shndx as usize)?;\n    Some((sym.value as i128, *link.bases.get(&site.section)? as i128))"),
   (OPS, "        let b = match &r.section { None => 0, Some(sec) => *link.bases.get(sec)? as i128 };\n        return Some((r.value as i128, b));",
        "        let b = match &r.section { None => 0, Some(_) => *link.bases.get(&site.section)? as i128 };\n        return Some((r.value as i128, b));")])

# ------------------------------------------------------------------ battery
v("b01_place_without_base", "ELF/addressing", "P = r_offset, the section base forgotten",
  [(OPS, "pub fn place(link: &Link, site: &Site) -> Option<i128> {\n    Some(*link.bases.get(&site.section)? as i128 + site.offset as i128)",
        "pub fn place(link: &Link, site: &Site) -> Option<i128> {\n    let _ = link.bases.get(&site.section)?;\n    Some(site.offset as i128)")])
v("b02_object_base_for_b", "ELF/addressing", "B = base of the object's first content section for every symbol",
  [(OPS, "    let sec = o.elf.sections.get(sym.shndx as usize)?;\n    Some((sym.value as i128, *link.bases.get(&sec.name)? as i128))",
        "    let _ = o.elf.sections.get(sym.shndx as usize)?;\n    let first = o.elf.sections.iter().find(|s| s.sh_type == 1)?;\n    Some((sym.value as i128, *link.bases.get(&first.name)? as i128))")])
v("b03_symbol_index_off_by_one", "ELF/addressing", "symbol index resolved without the null entry (off by one)",
  [(OPS, "    let sym = o.elf.symbols.get(site.symbol)?;\n    if sym.shndx == SHN_ABS {", "    let sym = o.elf.symbols.get(site.symbol.wrapping_sub(1))?;\n    if sym.shndx == SHN_ABS {")])
v("b04_rel_style_addend_ignored", "ELF/addressing", "REL-style processing: r_addend ignored (assumed to be in the field)",
  [(OPS, "                        out.push(Observation { s, a: site.addend as i128, p, b, object_word: site.object_word, outcome: outcome(link, site) });",
        "                        out.push(Observation { s, a: 0, p, b, object_word: site.object_word, outcome: outcome(link, site) });"),
   (PRED, "                            (Some(p), Some((s, b)), Some(pl)) => replay_single(p, s, site.addend as i128, pl, b, site.object_word),",
        "                            (Some(p), Some((s, b)), Some(pl)) => replay_single(p, s, 0, pl, b, site.object_word),")])
v("b05_addend_after_scale", "expression", "the addend is added after dividing by the scale",
  [(SEM, "pub fn value(expr: Expression, s: i128, a: i128, p: i128, b: i128) -> i128 {\n    match expr {",
        "pub fn value(expr: Expression, s: i128, a: i128, p: i128, b: i128) -> i128 {\n    let a = 0 * a;\n    match expr {"),
   (SEM, "    let q = scaled(value(p.expression, s, a, pl, b), p.scale);\n    encode(q, p.width(), p.signedness, p.overflow).map(|bits| place(bits, &p.slices, object_word))",
        "    let q = scaled(value(p.expression, s, a, pl, b), p.scale) + if matches!(p.expression, Expression::SPlusA | Expression::SPlusAMinusP | Expression::SPlusBPlusAMinusP) { a } else { 0 };\n    encode(q, p.width(), p.signedness, p.overflow).map(|bits| place(bits, &p.slices, object_word))"),
   (SEARCH, "            let comps: Vec<(i128, Outcome)> = obs.iter().map(|o| (scaled(value(expr, o.s, o.a, o.p, o.b), scale), o.outcome)).collect();",
        "            let comps: Vec<(i128, Outcome)> = obs.iter().map(|o| (scaled(value(expr, o.s, o.a, o.p, o.b), scale) + if matches!(expr, Expression::SPlusA | Expression::SPlusAMinusP | Expression::SPlusBPlusAMinusP) { o.a } else { 0 }, o.outcome)).collect();")])
v("b06_never_considers_b", "expression", "the search never considers the section base",
  [(SEARCH, "pub fn fit_single(r_type: u32, container: u32, obs: &[Observation]) -> Vec<Program> {\n    let index = Index::new(container, words_of(obs.iter().map(|o| (o.object_word, &o.outcome))));\n    let mut fits = Vec::new();\n    for expr in Expression::ALL {",
        "pub fn fit_single(r_type: u32, container: u32, obs: &[Observation]) -> Vec<Program> {\n    let index = Index::new(container, words_of(obs.iter().map(|o| (o.object_word, &o.outcome))));\n    let mut fits = Vec::new();\n    for expr in Expression::ALL.into_iter().filter(|e| *e != Expression::SPlusBPlusAMinusP) {")])
v("b07_place_after_scale", "expression", "P is subtracted after the division by the scale",
  [(SEM, "pub fn scaled(v: i128, scale: u32) -> i128 {\n    v / scale as i128\n}",
        "pub fn scaled(v: i128, scale: u32) -> i128 {\n    v / scale as i128\n}\npub fn scaled_pc(expr: Expression, s: i128, a: i128, p: i128, b: i128, scale: u32) -> i128 {\n    match expr {\n        Expression::SMinusP => scaled(s, scale) - p,\n        Expression::SPlusAMinusP => scaled(s + a, scale) - p,\n        Expression::SPlusBPlusAMinusP => scaled(s + b + a, scale) - p,\n        e => scaled(value(e, s, a, p, b), scale),\n    }\n}"),
   (SEM, "    let q = scaled(value(p.expression, s, a, pl, b), p.scale);\n    encode(q, p.width(), p.signedness, p.overflow).map(|bits| place(bits, &p.slices, object_word))",
        "    let q = scaled_pc(p.expression, s, a, pl, b, p.scale);\n    encode(q, p.width(), p.signedness, p.overflow).map(|bits| place(bits, &p.slices, object_word))"),
   (SEARCH, "use super::semantics::{encode, scaled, split, value};", "use super::semantics::{encode, scaled, scaled_pc, split, value};"),
   (SEARCH, "            let comps: Vec<(i128, Outcome)> = obs.iter().map(|o| (scaled(value(expr, o.s, o.a, o.p, o.b), scale), o.outcome)).collect();",
        "            let comps: Vec<(i128, Outcome)> = obs.iter().map(|o| (scaled_pc(expr, o.s, o.a, o.p, o.b, scale), o.outcome)).collect();")])
v("b09_scale_operands_first", "arithmetic", "each operand is divided by the scale before the expression is evaluated",
  [(SEM, "    let q = scaled(value(p.expression, s, a, pl, b), p.scale);\n    encode(q, p.width(), p.signedness, p.overflow).map(|bits| place(bits, &p.slices, object_word))",
        "    let sc = p.scale;\n    let q = value(p.expression, scaled(s, sc), scaled(a, sc), scaled(pl, sc), scaled(b, sc));\n    encode(q, p.width(), p.signedness, p.overflow).map(|bits| place(bits, &p.slices, object_word))"),
   (SEARCH, "            let comps: Vec<(i128, Outcome)> = obs.iter().map(|o| (scaled(value(expr, o.s, o.a, o.p, o.b), scale), o.outcome)).collect();",
        "            let comps: Vec<(i128, Outcome)> = obs.iter().map(|o| (value(expr, scaled(o.s, scale), scaled(o.a, scale), scaled(o.p, scale), scaled(o.b, scale)), o.outcome)).collect();")])
v("b10_exclusive_upper_bound", "arithmetic", "the top of the representable range is treated as exclusive",
  [(SEM, "    if x < lo || x > hi {", "    if x < lo || x >= hi {")])
v("b11_32bit_intermediate", "arithmetic", "the expression is evaluated in 32-bit two's-complement arithmetic",
  [(SEM, "pub fn value(expr: Expression, s: i128, a: i128, p: i128, b: i128) -> i128 {\n    match expr {",
        "pub fn value(expr: Expression, s: i128, a: i128, p: i128, b: i128) -> i128 {\n    let (s, a, p, b) = (s as i32 as i128, a as i32 as i128, p as i32 as i128, b as i32 as i128);\n    match expr {")])
v("b12_truncating_remainder_split", "arithmetic", "the low half of a pair uses the language remainder, negative for negative values",
  [(SEM, "    let lo = q.rem_euclid(m);\n    ((q - lo) / m, lo)", "    let lo = q % m;\n    ((q - lo) / m, lo)")])
v("b13_wrap_before_division", "arithmetic", "the value is wrapped to the field width before the division by the scale",
  [(SEM, "    let q = scaled(value(p.expression, s, a, pl, b), p.scale);\n    encode(q, p.width(), p.signedness, p.overflow).map(|bits| place(bits, &p.slices, object_word))",
        "    let w = p.width();\n    let v = value(p.expression, s, a, pl, b);\n    let v = if p.overflow == Overflow::Wrap { let m = 1i128 << w; let r = v.rem_euclid(m); if p.signedness == Signedness::Signed && r >= m / 2 { r - m } else { r } } else { v };\n    let q = scaled(v, p.scale);\n    encode(q, w, p.signedness, p.overflow).map(|bits| place(bits, &p.slices, object_word))"),
   (SEARCH, "                    match outcome {\n                        Outcome::Unknown => {}",
        "                    let x = &if ovf == Overflow::Wrap { let m = 1i128 << width; let r = (*x).rem_euclid(m); if sign == Signedness::Signed && r >= m / 2 { r - m } else { r } } else { *x };\n                    match outcome {\n                        Outcome::Unknown => {}")])
v("b14_signed_lower_bound_off_by_one", "arithmetic", "the signed range starts at -(2^(w-1)) + 1",
  [(SEM, "        Signedness::Signed => (-(1i128 << (width - 1)), (1i128 << (width - 1)) - 1),", "        Signedness::Signed => (-(1i128 << (width - 1)) + 1, (1i128 << (width - 1)) - 1),")])
v("b15_slices_sorted_by_lsb", "fields", "slices are reported in ascending physical order, not logical order",
  [(TOP, "    let predictions = predict::predict_all(corpus, &units, &programs);",
        "    for p in programs.values_mut() { p.slices.sort_by_key(|s| s.lsb); }\n    let predictions = predict::predict_all(corpus, &units, &programs);")])
v("b17_non_field_bits_zeroed", "fields", "the container word is rebuilt from the field alone; other bits are zeroed",
  [(SEM, "    encode(q, p.width(), p.signedness, p.overflow).map(|bits| place(bits, &p.slices, object_word))\n}\n\n/// Full replay of a pair instance",
        "    let _ = object_word;\n    encode(q, p.width(), p.signedness, p.overflow).map(|bits| place(bits, &p.slices, 0))\n}\n\n/// Full replay of a pair instance")])
v("b18_reversed_logical_order", "fields", "logical order of the slices reversed at output",
  [(TOP, "    let predictions = predict::predict_all(corpus, &units, &programs);",
        "    for p in programs.values_mut() { p.slices.reverse(); }\n    let predictions = predict::predict_all(corpus, &units, &programs);")])
v("b19_no_width_semantics", "fields", "the range check uses the container width instead of the field width",
  [(SEM, "    let q = scaled(value(p.expression, s, a, pl, b), p.scale);\n    encode(q, p.width(), p.signedness, p.overflow).map(|bits| place(bits, &p.slices, object_word))",
        "    let q = scaled(value(p.expression, s, a, pl, b), p.scale);\n    encode(q, 64, p.signedness, p.overflow).map(|bits| place(bits, &p.slices, object_word))"),
   (SEARCH, "                    match outcome {\n                        Outcome::Unknown => {}\n                        Outcome::Rejected => { if encode(*x, width, sign, ovf).is_some() { continue 'width; } }\n                        Outcome::Accepted => { if encode(*x, width, sign, ovf).is_none() { continue 'width; } }",
        "                    match outcome {\n                        Outcome::Unknown => {}\n                        Outcome::Rejected => { if encode(*x, 64, sign, ovf).is_some() { continue 'width; } }\n                        Outcome::Accepted => { if encode(*x, 64, sign, ovf).is_none() { continue 'width; } }")])
v("b20_overlapping_slices_allowed", "fields", "the layout walk does not exclude already-used container bits",
  [(SEARCH, "        for &j in compat[t as usize] {\n            if used >> j & 1 == 1 { continue; }", "        for &j in compat[t as usize] {")])
v("b21_truncating_pair_quotient", "pairs", "the high half is the truncating quotient, the low half the signed rest",
  [(SEM, "    let lo = q.rem_euclid(m);\n    ((q - lo) / m, lo)", "    let hi = q / m;\n    (hi, q - hi * m)")])
v("b22_members_fitted_independently", "pairs", "each pair member is fitted as an ordinary relocation with its own place",
  [(OPS, "                    Some(_) => { v.push(Unit::Pair(i, i + 1)); i += 2; }", "                    Some(_) => { v.push(Unit::Single(i)); v.push(Unit::Single(i + 1)); i += 2; }"),
   (TOP, "    for t in corpus.types.iter().filter(|t| t.pair_group.is_none()) {\n        let obs = operands::single_observations(corpus, &units, t.id);",
        "    for t in corpus.types.iter() {\n        let obs = operands::single_observations(corpus, &units, t.id);"),
   (TOP, "    for (group, mut members) in groups {", "    for (group, mut members) in groups.into_iter().filter(|_| false) {")])
v("b23_low_half_from_magnitude", "pairs", "the low half is taken from the absolute value",
  [(SEM, "    let lo = q.rem_euclid(m);\n    ((q - lo) / m, lo)", "    let lo = q.abs().rem_euclid(m);\n    ((q - lo) / m, lo)")])
v("b24_split_then_scale", "pairs", "the shared value is split before the division by the scale",
  [(SEM, "    let q = scaled(value(first.expression, s, a, pl, b), first.scale);\n    let (hi, lo) = split(q, k);",
        "    let (hi, lo) = split(value(first.expression, s, a, pl, b), k);\n    let (hi, lo) = (scaled(hi, first.scale), scaled(lo, first.scale));"),
   (SEARCH, "            let qs: Vec<i128> = obs.iter().map(|o| scaled(value(expr, o.s, o.a, o.p, o.b), scale)).collect();\n            for k in 1..=63u32 {\n                let parts: Vec<(i128, i128)> = qs.iter().map(|q| split(*q, k)).collect();",
        "            let qs: Vec<i128> = obs.iter().map(|o| value(expr, o.s, o.a, o.p, o.b)).collect();\n            for k in 1..=63u32 {\n                let parts: Vec<(i128, i128)> = qs.iter().map(|q| { let (h, l) = split(*q, k); (scaled(h, scale), scaled(l, scale)) }).collect();")])
v("b25_high_member_place", "pairs", "the pair's place is taken from the high member, whichever record comes first",
  [(PRED, "                            (Some(pf), Some(ps), Some((s, b)), Some(pl)) => replay_pair(pf, ps, s, first.addend as i128, pl, b, first.object_word, second.object_word),",
        "                            (Some(pf), Some(ps), Some((s, b)), Some(pl)) => { let pl = if pf.pair.as_ref().map(|q| q.role) == Some(crate::program::Role::Hi) { pl } else { place(link, second).unwrap_or(pl) }; replay_pair(pf, ps, s, first.addend as i128, pl, b, first.object_word, second.object_word) },"),
   (OPS, "                    if let (Some((s, b)), Some(p)) = (symbol_operands(o, link, first), place(link, first)) {\n                        let (lo_site, hi_site) = if first.r_type == lo_id { (first, second) } else { (second, first) };",
        "                    if let (Some((s, b)), Some(p)) = (symbol_operands(o, link, first), place(link, if first.r_type == hi_id { first } else { second })) {\n                        let (lo_site, hi_site) = if first.r_type == lo_id { (first, second) } else { (second, first) };")])
v("b27_majority_vote_across_objects", "global inference", "each object is fitted alone; the most frequent program wins",
  [(TOP, "        let obs = operands::single_observations(corpus, &units, t.id);\n        let fits = search::fit_single(t.id, t.container_bits, &obs);\n        let best = fits.into_iter().min_by_key(|p| p.key())\n            .ok_or_else(|| RecoverError(format!(\"no program in the grammar reproduces every observation of type {}\", t.id)))?;",
        "        let mut votes: BTreeMap<crate::program::CanonicalKey, (usize, Program)> = BTreeMap::new();\n        for (oi, _) in corpus.objects.iter().enumerate() {\n            let sub: Vec<Vec<operands::Unit>> = units.iter().enumerate().map(|(k, u)| if k == oi { u.clone() } else { Vec::new() }).collect();\n            let obs = operands::single_observations(corpus, &sub, t.id);\n            if obs.is_empty() { continue; }\n            if let Some(p) = search::fit_single(t.id, t.container_bits, &obs).into_iter().min_by_key(|p| p.key()) { votes.entry(p.key()).or_insert((0, p)).0 += 1; }\n        }\n        let best = votes.into_values().max_by_key(|(n, _)| *n).map(|(_, p)| p)\n            .ok_or_else(|| RecoverError(format!(\"no program in the grammar reproduces every observation of type {}\", t.id)))?;")])
v("b28_first_fit_in_enumeration_order", "global inference", "the first fitting program in enumeration order is reported",
  [(TOP, "        let best = fits.into_iter().min_by_key(|p| p.key())\n            .ok_or_else(|| RecoverError(format!(\"no program in the grammar reproduces every observation of type {}\", t.id)))?;",
        "        let best = fits.into_iter().next()\n            .ok_or_else(|| RecoverError(format!(\"no program in the grammar reproduces every observation of type {}\", t.id)))?;"),
   (TOP, "        let best = fits.into_iter().min_by_key(|(a, b)| (a.key(), b.key()))", "        let best = fits.into_iter().next()")])
v("b31_unsorted_output", "serialization", "programs and predictions emitted in discovery order",
  [(TOP, "    let mut result = Recovery { programs: programs.into_values().collect(), predictions };\n    result.sort();\n    Ok(result)",
        "    let mut programs: Vec<Program> = programs.into_values().collect();\n    programs.reverse();\n    let mut predictions = predictions;\n    predictions.reverse();\n    Ok(Recovery { programs, predictions })")])
v("b32_rejection_as_zero", "serialization", "a rejected relocation is predicted as the word 0 instead of null",
  [(PRED, "                        out.push(Prediction { object: o.id.clone(), link: link.id.clone(), section: site.section.clone(), offset: site.offset, word });",
        "                        out.push(Prediction { object: o.id.clone(), link: link.id.clone(), section: site.section.clone(), offset: site.offset, word: Some(word.unwrap_or(0)) });")])


def read(rel):
    return open(os.path.join(REF, rel)).read()


def main():
    shutil.rmtree(OUT, ignore_errors=True)
    names = sys.argv[1:] or list(VARIANTS)
    for name in names:
        family, desc, patches = VARIANTS[name]
        dst = os.path.join(OUT, name)
        os.makedirs(os.path.join(dst, "recover"))
        files = {rel: read(rel) for rel in [TOP, OPS, SEM, SEARCH, PRED]}
        for rel, old, new in patches:
            if files[rel].count(old) != 1:
                raise SystemExit("%s: pattern not unique in %s (%d)" % (name, rel, files[rel].count(old)))
            files[rel] = files[rel].replace(old, new)
        for rel, text in files.items():
            open(os.path.join(dst, rel), "w").write(text)
        open(os.path.join(dst, "VARIANT.txt"), "w").write("%s\n%s\n%s\n" % (name, family, desc))
    print("generated", len(names), "variants")


if __name__ == "__main__":
    main()
