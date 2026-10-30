import os, sys, json
B, R = sys.argv[1], sys.argv[2]
SRC = os.path.join(R, "solution/fixed/src")
def read(p): return open(os.path.join(SRC, p)).read()
FILES = {f: read(f) for f in ["solver.rs","solver/bounds.rs","solver/canonical.rs","solver/columns.rs","solver/path.rs","solver/progeny_dp.rs","solver/search.rs"]}
def mk(name, edits):
    d = os.path.join(B, name); os.makedirs(os.path.join(d, "solver"), exist_ok=True)
    files = dict(FILES)
    for f, old, new in edits:
        assert old in files[f], f"{name}: anchor missing in {f}:\n{old[:120]}"
        files[f] = files[f].replace(old, new, 1)
    for f, txt in files.items(): open(os.path.join(d, f), "w").write(txt)
    print("  built", name)
DR_DP = """                    double_reduction_term(cross, Parent::A, sa, m)
                        + double_reduction_term(cross, Parent::B, sb, m)"""
DR_PATH = """                        + double_reduction_term(cross, Parent::A, *sa, m)
                        + double_reduction_term(cross, Parent::B, *sb, m);"""
TR_DP = "mat[i * STATES + k] = transition_term(cross, parent, *from, *to, m);"
TR_PATH = """    transition_term(cross, Parent::A, ALL_STATES[ia], ALL_STATES[na], marker)
        + transition_term(cross, Parent::B, ALL_STATES[ib], ALL_STATES[nb], marker)"""
SET_FN = """
fn set_transition(cross: &Cross, parent: Parent, from: State, to: State, marker: usize) -> f64 {
    let r = cross.params(parent).recombination[marker];
    let mut f = vec![from.low(), from.high()]; f.dedup();
    let t = [to.low(), to.high()];
    let shared = f.iter().filter(|x| t.contains(x)).count() as f64;
    let k = 2.0 - shared;
    k * r.ln() + (2.0 - k) * (1.0 - r).ln()
}
"""
NODR_FN = """
fn dr_only_when_repeated(cross: &Cross, parent: Parent, state: State, marker: usize) -> f64 {
    if state.is_double_reduction() { cross.params(parent).dr_rate[marker].ln() } else { 0.0 }
}
"""
SOLVE_HEAD = "pub fn solve_counted(cross: &Cross) -> (Reconstruction, SearchStats) {"
BACKWARD = '''
fn backward_path(cross: &Cross, answer: &Reconstruction, progeny: usize) -> crate::cross_model::ProgenyPath {
    use crate::cross_model::{ProgenyPath, State, ALL_STATES, Parent};
    use crate::scoring::{double_reduction_term, progeny_dosage, transition_term};
    let joint: Vec<(State, State)> = ALL_STATES.iter().flat_map(|a| ALL_STATES.iter().map(move |b| (*a, *b))).collect();
    let legal = |sa: State, sb: State, m: usize| (!sa.is_double_reduction() || cross.params(Parent::A).dr_allowed[m]) && (!sb.is_double_reduction() || cross.params(Parent::B).dr_allowed[m]);
    let node_score = |sa: State, sb: State, m: usize| { let d = progeny_dosage(answer, sa, sb, m); cross.child_gl[progeny][m][usize::from(d)] + double_reduction_term(cross, Parent::A, sa, m) + double_reduction_term(cross, Parent::B, sb, m) };
    let mut best = vec![f64::NEG_INFINITY; joint.len()];
    let mut back: Vec<Vec<usize>> = vec![vec![usize::MAX; joint.len()]; cross.markers];
    for (i, (sa, sb)) in joint.iter().enumerate() { if legal(*sa, *sb, 0) { best[i] = node_score(*sa, *sb, 0); } }
    for m in 1..cross.markers {
        let mut next = vec![f64::NEG_INFINITY; joint.len()];
        for (i, (sa, sb)) in joint.iter().enumerate() {
            if !legal(*sa, *sb, m) { continue; }
            let here = node_score(*sa, *sb, m);
            for (j, (pa, pb)) in joint.iter().enumerate() {
                if best[j] == f64::NEG_INFINITY { continue; }
                let step = best[j] + transition_term(cross, Parent::A, *pa, *sa, m - 1) + transition_term(cross, Parent::B, *pb, *sb, m - 1);
                if step + here > next[i] { next[i] = step + here; back[m][i] = j; }
            }
        }
        best = next;
    }
    let mut cursor = 0; for i in 1..joint.len() { if best[i] > best[cursor] { cursor = i; } }
    let mut from_a = vec![ALL_STATES[0]; cross.markers]; let mut from_b = vec![ALL_STATES[0]; cross.markers];
    for m in (0..cross.markers).rev() { let (sa, sb) = joint[cursor]; from_a[m] = sa; from_b[m] = sb; if m > 0 { cursor = back[m][cursor]; } }
    ProgenyPath { from_a, from_b }
}
'''
PATHS_LOOP = """    for c in 0..cross.offspring {
        answer
            .paths
            .push(path::required_path(cross, c, &best.columns_a, &best.columns_b));
    }"""
SEARCH_BLOCK = """    let best = Search::new(
        cross,
        &terms,
        &bounds,
        &marker_bound,
        admissible_a.clone(),
        admissible_b.clone(),
    )
    .run();"""
mk("v01_marker_wise", [("solver.rs", SEARCH_BLOCK, """    let _ = (&bounds, &marker_bound);
    let mut columns_a: Vec<Column> = Vec::new();
    let mut columns_b: Vec<Column> = Vec::new();
    for m in 0..cross.markers {
        let dr = terms.dr_at(m);
        let mut pick: Option<(f64, Column, Column)> = None;
        for col_a in &admissible_a[m] {
            for col_b in &admissible_b[m] {
                let mut total = 0.0;
                for c in 0..cross.offspring {
                    let mut here = f64::NEG_INFINITY;
                    for j in 0..100 {
                        if dr[j] == f64::NEG_INFINITY { continue; }
                        let v = progeny_dp::emission(cross, c, m, j, col_a, col_b) + dr[j];
                        if v > here { here = v; }
                    }
                    total += here;
                }
                if pick.as_ref().map_or(true, |b| total > b.0) { pick = Some((total, *col_a, *col_b)); }
            }
        }
        let (_, ca, cb) = pick.expect("admissible column exists");
        columns_a.push(ca); columns_b.push(cb);
    }
    let best = search::BestAssignment { columns_a, columns_b, total: 0.0, stats: SearchStats::default() };""")])
mk("v02_fixed_guess", [("solver.rs", SEARCH_BLOCK, """    let _ = (&terms, &bounds, &marker_bound, &admissible_a, &admissible_b);
    let mut columns_a: Vec<Column> = Vec::new();
    let mut columns_b: Vec<Column> = Vec::new();
    for m in 0..cross.markers {
        let mut ca = [0u8; HOMOLOGS]; for h in 0..usize::from(cross.params(crate::cross_model::Parent::A).dosage[m]) { ca[h] = 1; }
        let mut cb = [0u8; HOMOLOGS]; for h in 0..usize::from(cross.params(crate::cross_model::Parent::B).dosage[m]) { cb[h] = 1; }
        columns_a.push(ca); columns_b.push(cb);
    }
    let best = search::BestAssignment { columns_a, columns_b, total: 0.0, stats: SearchStats::default() };""")])
mk("v03_set_intersection", [
    ("solver/progeny_dp.rs", TR_DP, "mat[i * STATES + k] = set_transition(cross, parent, *from, *to, m);"),
    ("solver/progeny_dp.rs", "use crate::scoring::{double_reduction_term, transition_term};", "use crate::scoring::double_reduction_term;" + SET_FN),
    ("solver/path.rs", TR_PATH, "    set_transition(cross, Parent::A, ALL_STATES[ia], ALL_STATES[na], marker)\n        + set_transition(cross, Parent::B, ALL_STATES[ib], ALL_STATES[nb], marker)"),
    ("solver/path.rs", "use crate::scoring::{double_reduction_term, transition_term};", "use crate::scoring::double_reduction_term;" + SET_FN),
])
mk("v04_omit_non_dr", [
    ("solver/progeny_dp.rs", DR_DP, "                    dr_only_when_repeated(cross, Parent::A, sa, m)\n                        + dr_only_when_repeated(cross, Parent::B, sb, m)"),
    ("solver/progeny_dp.rs", "use crate::scoring::{double_reduction_term, transition_term};", "use crate::scoring::transition_term;" + NODR_FN),
    ("solver/path.rs", DR_PATH, "                        + dr_only_when_repeated(cross, Parent::A, *sa, m)\n                        + dr_only_when_repeated(cross, Parent::B, *sb, m);"),
    ("solver/path.rs", "use crate::scoring::{double_reduction_term, transition_term};", "use crate::scoring::transition_term;" + NODR_FN),
])
mk("v05_penalised_forbidden_dr", [
    ("solver/progeny_dp.rs", """                row[j] = if legal_a && legal_b {
                    double_reduction_term(cross, Parent::A, sa, m)
                        + double_reduction_term(cross, Parent::B, sb, m)
                } else {
                    f64::NEG_INFINITY
                };""", """                let term = |p: Parent, s: State, ok: bool| if ok { double_reduction_term(cross, p, s, m) } else if s.is_double_reduction() { -3.0 } else { 0.0 };
                row[j] = term(Parent::A, sa, legal_a) + term(Parent::B, sb, legal_b);"""),
    ("solver/path.rs", """                if sa.is_double_reduction() && !cross.params(Parent::A).dr_allowed[m] {
                    continue;
                }
""", ""),
    ("solver/path.rs", """                    if sb.is_double_reduction() && !cross.params(Parent::B).dr_allowed[m] {
                        continue;
                    }
""", ""),
    ("solver/path.rs", DR_PATH, """                        + (if sa.is_double_reduction() && !cross.params(Parent::A).dr_allowed[m] { -3.0 } else { double_reduction_term(cross, Parent::A, *sa, m) })
                        + (if sb.is_double_reduction() && !cross.params(Parent::B).dr_allowed[m] { -3.0 } else { double_reduction_term(cross, Parent::B, *sb, m) });"""),
])
mk("v06_backward_traceback", [
    ("solver.rs", PATHS_LOOP, """    for c in 0..cross.offspring {
        let p = backward_path(cross, &answer, c);
        answer.paths.push(p);
    }
    let _ = path::required_path;"""),
    ("solver.rs", "/// Assembles one parent's haplotype rows from its per marker columns.", BACKWARD + "\n/// Assembles one parent's haplotype rows from its per marker columns."),
])
mk("v07_rows_only_canonical", [("solver/canonical.rs", "    if sorted_rows.len() == 1 {\n        return sorted_rows[0];\n    }", "    if !sorted_rows.is_empty() {\n        return sorted_rows[0];\n    }")])
mk("v08_joint_canonical", [("solver/canonical.rs", """    for parent in Parent::BOTH {
        let perm = choose_permutation(answer, parent);
        apply(answer, parent, &perm);
    }""", """    let perm = choose_permutation(answer, Parent::A);
    apply(answer, Parent::A, &perm);
    apply(answer, Parent::B, &perm);""")])
mk("v09_wrong_perm_direction", [("solver/canonical.rs", "    let remapped = answer.haplotypes(parent).remap(perm);", """    let mut inv = [0u8; HOMOLOGS];
    for h in 0..HOMOLOGS { inv[perm[h] as usize] = h as u8; }
    let remapped = answer.haplotypes(parent).remap(&inv);""")])
mk("v10_interleaved_path_order", [("solver/path.rs", """    let layers = Layers::build(cross, progeny, columns_a, columns_b);
    let from_a = smallest_parent_a(cross, &layers);
    let from_b = smallest_parent_b(cross, &layers, &from_a);
    ProgenyPath { from_a, from_b }""", """    let layers = Layers::build(cross, progeny, columns_a, columns_b);
    let mut ca: Vec<usize> = Vec::new(); let mut cb: Vec<usize> = Vec::new(); let mut running = 0.0f64;
    for m in 0..cross.markers {
        let mut settled = None;
        'outer: for ia in 0..STATES { for ib in 0..STATES {
            if layers.node[m][ia][ib] == f64::NEG_INFINITY { continue; }
            let value = if m == 0 { layers.node[0][ia][ib] } else { running + interval(cross, ca[m-1], cb[m-1], ia, ib, m-1) + layers.node[m][ia][ib] };
            if value + layers.back[m][ia][ib] >= layers.optimum - REACH { settled = Some((ia, ib, value)); break 'outer; }
        }}
        let (ia, ib, value) = settled.expect("some joint state lies on an optimal path");
        ca.push(ia); cb.push(ib); running = value;
    }
    let _ = (smallest_parent_a, smallest_parent_b);
    ProgenyPath { from_a: ca.into_iter().map(|i| ALL_STATES[i]).collect(), from_b: cb.into_iter().map(|i| ALL_STATES[i]).collect() }""")])
mk("v11_no_canonical", [("solver.rs", "    canonicalise(&mut answer);\n    (answer, best.stats)", "    let _ = canonicalise;\n    (answer, best.stats)")])
mk("v15_non_admissible_bound", [("solver/bounds.rs", "    pub fn after(&self, marker: usize) -> f64 {\n        self.suffix[marker]\n    }", "    pub fn after(&self, marker: usize) -> f64 {\n        self.suffix[marker] - 0.75\n    }")])
mk("v16_prune_ties", [
    ("solver/search.rs", "                if optimistic < best.total - TIE_TOLERANCE {\n                    self.stats.pruned += 1;\n                    continue;\n                }", "                if optimistic <= best.total + TIE_TOLERANCE {\n                    self.stats.pruned += 1;\n                    continue;\n                }"),
    ("solver/search.rs", "            if optimistic < best.total - TIE_TOLERANCE {\n                return None;\n            }", "            if optimistic <= best.total + TIE_TOLERANCE {\n                return None;\n            }"),
])
mk("v17_float_equality", [("solver/path.rs", "const REACH: f64 = 1e-7;", "const REACH: f64 = 0.0;")])
mk("v18_first_found_tie", [("solver/search.rs", "                    row_major_less(&self.chosen_a, &self.chosen_b, &best.columns_a, &best.columns_b)", "                    { let _ = row_major_less; false }")])
exp = json.load(open(os.path.join(R, "tests/fixtures/baseline.expected.json")))
def rows(P): return ", ".join("vec![%s]" % ", ".join(str(v) for v in r) for r in exp["haplotypes"][P])
def path(p, P): return ", ".join("State::new(%d, %d).unwrap()" % (a, b) for a, b in p[P])
paths = ",\n        ".join("ProgenyPath { from_a: vec![%s], from_b: vec![%s] }" % (path(p, "A"), path(p, "B")) for p in exp["paths"])
mk("v20_hardcoded_visible_cross", [("solver.rs", SOLVE_HEAD, SOLVE_HEAD + """
    use crate::cross_model::{ProgenyPath, State};
    let _ = cross;
    let answer = Reconstruction {
        haplotypes_a: Haplotypes::new([%s]),
        haplotypes_b: Haplotypes::new([%s]),
        paths: vec![
        %s
        ],
    };
    return (answer, SearchStats::default());
}
fn _unused(cross: &Cross) -> (Reconstruction, SearchStats) {
    let _u = (RemainingBound::build, MarkerBound::build, StaticTerms::build, admissible_columns, canonicalise);""" % (rows("A"), rows("B"), paths))])

NOSYM = [
    ("solver/columns.rs", "    pub fn extend(self, column: &Column) -> Option<RowOrder> {", "    pub fn extend(self, _column: &Column) -> Option<RowOrder> {\n        return Some(self);\n    }\n    #[allow(dead_code)]\n    fn extend_unused(self, column: &Column) -> Option<RowOrder> {"),
    ("solver/search.rs", "                    row_major_less(&self.chosen_a, &self.chosen_b, &best.columns_a, &best.columns_b)", "                    { let _ = row_major_less; false }"),
]
mk("v06b_nosym_backward_traceback", NOSYM + [
    ("solver.rs", PATHS_LOOP, """    for c in 0..cross.offspring {
        let p = backward_path(cross, &answer, c);
        answer.paths.push(p);
    }
    let _ = path::required_path;"""),
    ("solver.rs", "/// Assembles one parent's haplotype rows from its per marker columns.", BACKWARD + "\n/// Assembles one parent's haplotype rows from its per marker columns."),
])
mk("v07b_nosym_rows_only_canonical", NOSYM + [("solver/canonical.rs", "    if sorted_rows.len() == 1 {\n        return sorted_rows[0];\n    }", "    if !sorted_rows.is_empty() {\n        return sorted_rows[0];\n    }")])
mk("v10b_nosym_interleaved_path_order", NOSYM + [("solver/path.rs", """    let layers = Layers::build(cross, progeny, columns_a, columns_b);
    let from_a = smallest_parent_a(cross, &layers);
    let from_b = smallest_parent_b(cross, &layers, &from_a);
    ProgenyPath { from_a, from_b }""", """    let layers = Layers::build(cross, progeny, columns_a, columns_b);
    let mut ca: Vec<usize> = Vec::new(); let mut cb: Vec<usize> = Vec::new(); let mut running = 0.0f64;
    for m in 0..cross.markers {
        let mut settled = None;
        'outer: for ia in 0..STATES { for ib in 0..STATES {
            if layers.node[m][ia][ib] == f64::NEG_INFINITY { continue; }
            let value = if m == 0 { layers.node[0][ia][ib] } else { running + interval(cross, ca[m-1], cb[m-1], ia, ib, m-1) + layers.node[m][ia][ib] };
            if value + layers.back[m][ia][ib] >= layers.optimum - REACH { settled = Some((ia, ib, value)); break 'outer; }
        }}
        let (ia, ib, value) = settled.expect("some joint state lies on an optimal path");
        ca.push(ia); cb.push(ib); running = value;
    }
    let _ = (smallest_parent_a, smallest_parent_b);
    ProgenyPath { from_a: ca.into_iter().map(|i| ALL_STATES[i]).collect(), from_b: cb.into_iter().map(|i| ALL_STATES[i]).collect() }""")])
mk("v11b_nosym_no_canonical", NOSYM + [("solver.rs", "    canonicalise(&mut answer);\n    (answer, best.stats)", "    let _ = canonicalise;\n    (answer, best.stats)")])
mk("v22_nosym_full_canonical", NOSYM)

# Post-search relabelling: score-invariant, zero cost. Simulates a search that
# happened to label the homologs differently, so the canonicalisation step has
# real work to do instead of receiving pre-sorted rows.
SCRAMBLE = [("solver.rs", SEARCH_BLOCK, SEARCH_BLOCK + """
    let scramble = |cols: &Vec<Column>| -> Vec<Column> {
        cols.iter().map(|c| { let p = [2usize, 3, 0, 1]; let mut o = [0u8; HOMOLOGS]; for h in 0..HOMOLOGS { o[p[h]] = c[h]; } o }).collect()
    };
    let best = search::BestAssignment { columns_a: scramble(&best.columns_a), columns_b: scramble(&best.columns_b), total: best.total, stats: best.stats };""")]
mk("v22s_scramble_full_canonical", SCRAMBLE)
mk("v07s_scramble_rows_only_canonical", SCRAMBLE + [("solver/canonical.rs", "    if sorted_rows.len() == 1 {\n        return sorted_rows[0];\n    }", "    if !sorted_rows.is_empty() {\n        return sorted_rows[0];\n    }")])
mk("v08s_scramble_joint_canonical", SCRAMBLE + [("solver/canonical.rs", """    for parent in Parent::BOTH {
        let perm = choose_permutation(answer, parent);
        apply(answer, parent, &perm);
    }""", """    let perm = choose_permutation(answer, Parent::A);
    apply(answer, Parent::A, &perm);
    apply(answer, Parent::B, &perm);""")])
mk("v09s_scramble_wrong_perm_direction", SCRAMBLE + [("solver/canonical.rs", "    let remapped = answer.haplotypes(parent).remap(perm);", """    let mut inv = [0u8; HOMOLOGS];
    for h in 0..HOMOLOGS { inv[perm[h] as usize] = h as u8; }
    let remapped = answer.haplotypes(parent).remap(&inv);""")])
mk("v11s_scramble_no_canonical", SCRAMBLE + [("solver.rs", "    canonicalise(&mut answer);\n    (answer, best.stats)", "    let _ = canonicalise;\n    (answer, best.stats)")])
# first-found among tied assignments, with siblings expanded in reverse order so
# discovery order differs from the reference's
mk("v18b_first_found_tie_reversed", [
    ("solver/search.rs", "                    row_major_less(&self.chosen_a, &self.chosen_b, &best.columns_a, &best.columns_b)", "                    { let _ = row_major_less; false }"),
    ("solver/search.rs", 'children.sort_by(|l, r| r.0.partial_cmp(&l.0).expect("scores are finite"));', 'children.sort_by(|l, r| r.0.partial_cmp(&l.0).expect("scores are finite"));\n        children.reverse();'),
])

# Scramble, then put the rows back into canonical order BEFORE path recovery, so
# the lex-min recovery runs in canonical label space. This is the correct
# ordering and must score 1: it validates the scramble mechanism itself.
mk("v22c_scramble_then_sort_rows_first", SCRAMBLE + [("solver.rs", """    let mut answer = Reconstruction {
        haplotypes_a: haplotypes_from_columns(&best.columns_a),
        haplotypes_b: haplotypes_from_columns(&best.columns_b),
        paths: Vec::with_capacity(cross.offspring),
    };""", """    let sort_cols = |cols: &Vec<Column>| -> Vec<Column> {
        let rows: Vec<Vec<u8>> = (0..HOMOLOGS).map(|h| cols.iter().map(|c| c[h]).collect()).collect();
        let mut idx: Vec<usize> = (0..HOMOLOGS).collect();
        idx.sort_by(|a, b| rows[*a].cmp(&rows[*b]));
        cols.iter().map(|c| { let mut o = [0u8; HOMOLOGS]; for (new, old) in idx.iter().enumerate() { o[new] = c[*old]; } o }).collect()
    };
    let best = search::BestAssignment { columns_a: sort_cols(&best.columns_a), columns_b: sort_cols(&best.columns_b), total: best.total, stats: best.stats };
    let mut answer = Reconstruction {
        haplotypes_a: haplotypes_from_columns(&best.columns_a),
        haplotypes_b: haplotypes_from_columns(&best.columns_b),
        paths: Vec::with_capacity(cross.offspring),
    };""")])

print("variants:", len(os.listdir(B)))
