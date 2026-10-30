#!/usr/bin/env python3
"""Author-side implementation snapshots S0..S7: staged versions of the
reference that mirror a plausible development history. Each snapshot is the
reference with the capabilities that a later phase adds still missing.

  S0 starter                 the shipped placeholder
  S1 scalar replay           operands + replay; a single guessed contiguous
                             field, scale 1, signed, reject; first object only
  S2 first inference         full scalar grammar, contiguous field, per-object,
                             no pair handling, first fit
  S3 fields                  multi-slice layouts, still per-object, no pairs
  S4 cross-object            global consistency over all objects, no pairs
  S5 pairs                   pair coupling implemented, first fit
  S6 canonical               canonical key, failure records still ignored
  S7 final                   the reference
"""
import os, shutil, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from make_variants import VARIANTS, REF, TOP, OPS, SEM, SEARCH, PRED, read

OUT = os.path.join(HERE, "snapshots")

SKIP_UNFIT = [
    (TOP, "        let best = fits.into_iter().min_by_key(|p| p.key())\n            .ok_or_else(|| RecoverError(format!(\"no program in the grammar reproduces every observation of type {}\", t.id)))?;\n        programs.insert(t.id, best);",
          "        if let Some(best) = fits.into_iter().min_by_key(|p| p.key()) { programs.insert(t.id, best); }"),
]
FIRST_FIT = VARIANTS["b28_first_fit_in_enumeration_order"][2]
PER_OBJECT = VARIANTS["p03_local_fit_first_object"][2]
CONTIGUOUS = VARIANTS["p04_contiguous_field_only"][2]
NO_PAIRS = VARIANTS["b22_members_fitted_independently"][2]
SCALAR_ONLY = [
    (SEARCH, "    for expr in Expression::ALL {\n        for scale in SCALES {\n            let comps: Vec<(i128, Outcome)>",
             "    for expr in Expression::ALL {\n        for scale in [1u32] {\n            let comps: Vec<(i128, Outcome)>"),
    (SEARCH, "    for sign in [Signedness::Unsigned, Signedness::Signed] {\n        for ovf in [Overflow::Reject, Overflow::Wrap] {",
             "    for sign in [Signedness::Signed] {\n        for ovf in [Overflow::Reject] {"),
]
IGNORE_FAILURES = [
    (OPS, "        LinkResult::Failed { .. } => if link.rejected(&site.section, site.offset) == Some(true) { Outcome::Rejected } else { Outcome::Accepted },",
          "        LinkResult::Failed { .. } => Outcome::Unknown,"),
]

def first_fit_skip_unfit():
    # first-fit variant of the skip-unfit patch (the patch texts must chain)
    return [
        (TOP, "        let best = fits.into_iter().min_by_key(|p| p.key())\n            .ok_or_else(|| RecoverError(format!(\"no program in the grammar reproduces every observation of type {}\", t.id)))?;\n        programs.insert(t.id, best);",
              "        if let Some(best) = fits.into_iter().next() { programs.insert(t.id, best); }"),
        (TOP, "        let best = fits.into_iter().min_by_key(|(a, b)| (a.key(), b.key()))", "        let best = fits.into_iter().next()"),
    ]

BEST_EFFORT = [
    (TOP, "use crate::corpus::Corpus;\nuse crate::program::{Program, Recovery};",
          "use crate::corpus::Corpus;\nuse crate::program::{Program, Recovery, Expression, Signedness, Overflow, Slice, SCALES};\n\n/// Early-development fallback: when nothing fits exactly, keep the hypothesis that\n/// reproduces the most observed words, with one contiguous field over the changed bits.\nfn best_effort(r_type: u32, container: u32, obs: &[operands::Observation], scales: &[u32], signs: &[Signedness], ovfs: &[Overflow]) -> Option<Program> {\n    let words: Vec<(u64, u64)> = obs.iter().filter_map(|o| if let operands::Outcome::Word(w) = o.outcome { Some((o.object_word, w)) } else { None }).collect();\n    if words.is_empty() { return None; }\n    let mask = words.iter().fold(0u64, |m, (a, b)| m | (a ^ b));\n    if mask == 0 { return None; }\n    let lsb = mask.trailing_zeros();\n    let _ = container; let width = 64 - lsb - mask.leading_zeros();\n    let slices = vec![Slice { lsb, width }];\n    let mut best: Option<(usize, Program)> = None;\n    for expr in Expression::ALL { for &scale in scales { for &sign in signs { for &ovf in ovfs {\n        let p = Program { r_type, expression: expr, scale, signedness: sign, overflow: ovf, slices: slices.clone(), pair: None };\n        let hits = obs.iter().filter(|o| matches!(o.outcome, operands::Outcome::Word(w) if semantics::replay_single(&p, o.s, o.a, o.p, o.b, o.object_word) == Some(w))).count();\n        if best.as_ref().map(|(h, _)| hits > *h).unwrap_or(true) { best = Some((hits, p)); }\n    } } } }\n    best.map(|(_, p)| p)\n}"),
]
S1_FALLBACK = [
    (TOP, "        if let Some(best) = fits.into_iter().next() { programs.insert(t.id, best); }",
          "        if let Some(best) = fits.into_iter().next() { programs.insert(t.id, best); } else if let Some(p) = best_effort(t.id, t.container_bits, &obs, &[1], &[Signedness::Signed], &[Overflow::Reject]) { programs.insert(t.id, p); }"),
]
S2_FALLBACK = [
    (TOP, "        if let Some(best) = fits.into_iter().next() { programs.insert(t.id, best); }",
          "        if let Some(best) = fits.into_iter().next() { programs.insert(t.id, best); } else if let Some(p) = best_effort(t.id, t.container_bits, &obs, &SCALES, &[Signedness::Unsigned, Signedness::Signed], &[Overflow::Reject, Overflow::Wrap]) { programs.insert(t.id, p); }"),
]

SNAPSHOTS = {
    "S1_scalar_replay": SCALAR_ONLY + CONTIGUOUS + PER_OBJECT + NO_PAIRS + first_fit_skip_unfit() + BEST_EFFORT + S1_FALLBACK,
    "S2_first_inference": CONTIGUOUS + PER_OBJECT + NO_PAIRS + first_fit_skip_unfit() + BEST_EFFORT + S2_FALLBACK,
    "S3_fields": PER_OBJECT + NO_PAIRS + first_fit_skip_unfit() + BEST_EFFORT + S2_FALLBACK,
    "S4_cross_object": NO_PAIRS + first_fit_skip_unfit() + BEST_EFFORT + S2_FALLBACK,
    "S5_pairs": FIRST_FIT,
    "S6_canonical_no_failure_records": IGNORE_FAILURES,
    "S7_final": [],
}

def main():
    shutil.rmtree(OUT, ignore_errors=True)
    for name, patches in SNAPSHOTS.items():
        dst = os.path.join(OUT, name)
        os.makedirs(os.path.join(dst, "recover"))
        files = {rel: read(rel) for rel in [TOP, OPS, SEM, SEARCH, PRED]}
        for rel, old, new in patches:
            if files[rel].count(old) != 1:
                raise SystemExit("%s: pattern not unique in %s (%d)" % (name, rel, files[rel].count(old)))
            files[rel] = files[rel].replace(old, new)
        for rel, text in files.items():
            open(os.path.join(dst, rel), "w").write(text)
    # S0 is the starter itself
    os.makedirs(os.path.join(OUT, "S0_starter"))
    shutil.copy(os.path.join(REF, "..", "..", "..", "environment", "src", "recover.rs"), os.path.join(OUT, "S0_starter", "recover.rs"))
    print("snapshots:", sorted(os.listdir(OUT)))

if __name__ == "__main__":
    main()
