#!/usr/bin/env python3
"""Single-mistake variants and plausible incomplete designs, each a solver tree
(solver.rs + solver/) derived from the reference by a textual patch. A patch
that fails to apply is an error, so a variant never silently equals the
reference. Author-side."""
import os, sys, shutil
W = os.path.expanduser('~/benchmark-suite-work/plasmid-map-deccan')
REF = os.path.join(W, 'pkg/solution/fixed/src')
OUT = os.path.join(W, 'adversarial/variants')

V = {}
def v(name, desc, patches): V[name] = (desc, patches)

# ---- single mistakes (b) --------------------------------------------------
v('b01_tolerance_floor', 'tolerance floor(e*b/100) instead of ceil, in every bound', [
  ('bnb.rs', 'use crate::digest;', 'use crate::digest;\nfn tolerance(b: u32, e: u32) -> u32 { (u64::from(e) * u64::from(b) / 100) as u32 }'),
  ('bnb.rs', 'digest::tolerance(', 'tolerance(', 'all')])
v('b02_open_insert_region', 'insert-region positions strictly inside (c, c+L)', [
  ('bnb.rs', "edges.push((i + 1, zero, -c)); // box >= c", "edges.push((i + 1, zero, -c - 1)); // box > c"),
  ('bnb.rs', "edges.push((0, i + 1, c)); // box - L <= c", "edges.push((0, i + 1, c - 1)); // box - L < c")])
v('b03_junction_windows_kept', 'backbone windows torn by the insertion are kept as required sites', [
  ('bnb.rs', "all.iter().filter(|s| !sites::spans_junction(s.start, enzyme.site.len(), c, v)).map(|s| s.cut).collect();", "all.iter().map(|s| s.cut).collect();")])
v('b04_shift_only_above_site', 'a backbone site at the cloning site itself does not move', [
  ('bnb.rs', "let a: Vec<usize> = surviving.iter().copied().filter(|&p| p < c).collect();\n            let b: Vec<usize> = surviving.iter().copied().filter(|&p| p >= c).collect();", "let a: Vec<usize> = surviving.iter().copied().filter(|&p| p <= c).collect();\n            let b: Vec<usize> = surviving.iter().copied().filter(|&p| p > c).collect();")])
v('b05_sub_detection_counted', 'fragments below the detection limit are treated as visible in the search', [
  ('bnb.rs', "let d = problem.detection_limit as i64;\n        let e = problem.error_percent;\n        let nvars", "let d = 1i64;\n        let e = problem.error_percent;\n        let nvars"),
  ('bnb.rs', "let d = self.problem.detection_limit as i64;\n        let e = self.problem.error_percent;\n        let mut out = Vec::new();", "let d = 1i64;\n        let e = self.problem.error_percent;\n        let mut out = Vec::new();")])
v('b06_multiplicity_ignored', 'a band counts once whatever its multiplicity', [
  ('bnb.rs', "let expanded = digest::expand(&dg.bands);\n                let mut distinct = expanded.clone();\n                distinct.dedup();", "let mut expanded = digest::expand(&dg.bands);\n                expanded.dedup();\n                let distinct = expanded.clone();")])
v('b07_ties_by_site_then_length', 'ties on count and deviation broken by cloning site and insert length before positions', [
  ('bnb.rs', "let better = match best {\n            Some(b) => k < b.key,\n            None => true,\n        };", "let better = match best {\n            Some(b) => (k.internal, k.deviation, k.cloning_site, k.insert_length, k.positions.clone()) < (b.key.internal, b.key.deviation, b.key.cloning_site, b.key.insert_length, b.key.positions.clone()),\n            None => true,\n        };")])
v('b08_minimum_count_assumed', 'only the least site count each single digest admits is searched', [
  ('solver.rs', "for t in minimum[i]..=k {", "for t in minimum[i]..=minimum[i] {")])
v('b09_first_cloning_site_only', 'only the first listed cloning site is considered', [
  ('solver.rs', "let contexts: Vec<bnb::SiteContext> = problem.cloning_sites.iter().map(|s| bnb::SiteContext::new(problem, s.position)).collect();", "let contexts: Vec<bnb::SiteContext> = problem.cloning_sites.iter().take(1).map(|s| bnb::SiteContext::new(problem, s.position)).collect();")])
v('b10_first_map_found', 'the search stops at the first map that reproduces the gels', [
  ('bnb.rs', "        let better = match best {\n            Some(b) => k < b.key,\n            None => true,\n        };", "        let better = best.is_none();")])
v('b11_deviation_before_count', 'the closest fit wins across site counts', [
  ('solver.rs', "        if let Some(b) = best {\n            return b.map;\n        }\n    }\n    fallback(problem)", "        if let Some(b) = best {\n            if overall.as_ref().map_or(true, |o: &bnb::Best| b.key.deviation < o.key.deviation) { overall = Some(b); }\n        }\n    }\n    if let Some(o) = overall { return o.map; }\n    fallback(problem)"),
  ('solver.rs', "    let start = contexts.iter().map(|ctx| ctx.minimum.iter().sum::<usize>()).min().unwrap_or(0);", "    let start = contexts.iter().map(|ctx| ctx.minimum.iter().sum::<usize>()).min().unwrap_or(0);\n    let mut overall: Option<bnb::Best> = None;")])
v('b12_wrap_fragment_dropped', 'the wrap-around fragment is not part of a digest', [
  ('bnb.rs', "        out.push(Frag { plus: cuts[0], minus: cuts[cuts.len() - 1], wrap: true });\n        out", "        out")])
v('b13_prune_at_equality', 'a box whose bound equals the incumbent is discarded', [
  ('bnb.rs', "            if lb > b.key.deviation {\n                return;\n            }\n        }\n        if s.l.width() == 0", "            if lb >= b.key.deviation {\n                return;\n            }\n        }\n        if s.l.width() == 0")])
v('b14_held_out_unfiltered', 'held-out predictions list fragments below the detection limit', [
  ('bnb.rs', "        let held_out = predictions(problem, insert_length, &cuts);\n        let map = PlasmidMap { cloning_site: c, insert_length, cuts, held_out };\n        let k = key(&map, total);", "        let mut held_out = predictions(problem, insert_length, &cuts);\n        for h in &problem.held_out { held_out.insert(h.id.clone(), digest::group(&digest::predicted(&cuts, &h.enzymes, n, 1))); }\n        let map = PlasmidMap { cloning_site: c, insert_length, cuts, held_out };\n        let k = key(&map, total);")])

# ---- plausible incomplete designs (p) -------------------------------------
v('p01_bisection_only', 'branch and bound that only halves intervals: no branching on band windows or cut order', [
  ('bnb.rs', "        if let Some((di, fi, iv)) = self.unsettled(&s, constraints) {", "        if let Some((di, fi, iv)) = None::<(usize, usize, Interval)> {"),
  ('bnb.rs', "                let narrow = x.width() <= 32 && y.width() <= 32;", "                let narrow = false && x.width() <= 32 && y.width() <= 32;")])
v('p02_no_tangent_bound', 'deviation bounded only by assignments and order statistics, never by the quadratic', [
  ('bnb.rs', "        if structured.is_empty() {\n            return Some(loose);\n        }", "        if true || structured.is_empty() {\n            return Some(loose);\n        }")])
v('p03_no_order_branching', 'overlapping cuts of different enzymes are only separated by halving', [
  ('bnb.rs', "                let narrow = x.width() <= 32 && y.width() <= 32;", "                let narrow = false && x.width() <= 32 && y.width() <= 32;")])
v('p04_no_visibility_windows', 'visibility counts never constrain borderline fragments', [
  ('bnb.rs', "                if iv.lo < d && iv.hi >= d {\n                    if possible == m {", "                if false && iv.lo < d && iv.hi >= d {\n                    if possible == m {")])
v('p05_interval_propagation_only', 'constraints propagated by interval arithmetic for eight rounds instead of shortest paths', [
  ('bnb.rs', "        for _round in 0..12 {\n            let before = s.clone();\n            let Some(windows) = self.windows(s, constraints) else {\n                return false;\n            };", "        for _round in 0..8 {\n            let before = s.clone();\n            let Some(windows) = self.windows(s, constraints) else {\n                return false;\n            };\n            for (f, lo, hi) in &windows { if !self.tighten(f, *lo, *hi, s) { return false; } }\n            if s.l == before.l && s.boxes == before.boxes { break; }\n            continue;")])


def apply(text, old, new, mode, name):
    if old not in text:
        raise SystemExit("variant %s: patch anchor not found: %r" % (name, old[:60]))
    return text.replace(old, new) if mode == 'all' else text.replace(old, new, 1)


def main():
    shutil.rmtree(OUT, ignore_errors=True)
    for name, (desc, patches) in V.items():
        d = os.path.join(OUT, name)
        shutil.copytree(REF, d)
        for patch in patches:
            file, old, new = patch[0], patch[1], patch[2]
            mode = patch[3] if len(patch) > 3 else 'one'
            path = os.path.join(d, 'solver', file) if file != 'solver.rs' else os.path.join(d, 'solver.rs')
            text = open(path).read()
            text = apply(text, old, new, mode, name)
            open(path, 'w').write(text)
        open(os.path.join(d, 'DESCRIPTION.txt'), 'w').write(desc + '\n')
    print("variants written:", len(V))


if __name__ == '__main__':
    main()
