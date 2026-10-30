//! Scratch harness: random crosses, brute force, and checks of the search.
//!
//! Not part of the deliverable; used while developing the solver.

use polyploid_phase::cross_input::{parse_cross, Cross};
use polyploid_phase::cross_model::{
    Haplotypes, Parent, Reconstruction, ALL_STATES,
};
use polyploid_phase::scoring::score;
use polyploid_phase::solver::bb::Search;
use polyploid_phase::solver::canon::haplotypes_of;
use polyploid_phase::solver::local::{Local, Rng};
use polyploid_phase::solver::tables::Tables;
use polyploid_phase::solver::viterbi::{Chain, SuffixBounds};
use polyploid_phase::solver::solve;

struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % (n as u64)) as usize
    }
    fn float(&mut self) -> f64 {
        ((self.next() >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

/// Builds a random cross document.
///
/// `free` counts how many markers of each parent get a dosage that leaves more
/// than one admissible column, which is what makes the haplotype space large.
fn random_cross(seed: u64, markers: usize, offspring: usize, free: usize, flat: bool) -> String {
    let mut g = Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut dose = ([0u8; 32], [0u8; 32]);
    for p in 0..2 {
        let mut slots: Vec<usize> = (0..markers).collect();
        for i in (1..slots.len()).rev() {
            let j = g.below(i + 1);
            slots.swap(i, j);
        }
        for (i, m) in slots.iter().enumerate() {
            let d = if i < free { 1 + g.below(3) } else { if g.below(2) == 0 { 0 } else { 4 } };
            if p == 0 { dose.0[*m] = d as u8; } else { dose.1[*m] = d as u8; }
        }
    }
    // A planted truth: random columns consistent with the dosages.
    let mut truth = ([[0u8; 32]; 4], [[0u8; 32]; 4]);
    for p in 0..2 {
        for m in 0..markers {
            let d = if p == 0 { dose.0[m] } else { dose.1[m] };
            let mut homologs: Vec<usize> = (0..4).collect();
            for i in (1..4).rev() {
                let j = g.below(i + 1);
                homologs.swap(i, j);
            }
            for h in 0..d as usize {
                if p == 0 { truth.0[homologs[h]][m] = 1; } else { truth.1[homologs[h]][m] = 1; }
            }
        }
    }
    // Planted paths and the dosages they imply.
    let mut gl = String::new();
    let mut first_progeny = true;
    gl.push('[');
    for _c in 0..offspring {
        let mut first_marker = true;
        if !first_progeny { gl.push(','); }
        first_progeny = false;
        gl.push('[');
        for m in 0..markers {
            let sa = ALL_STATES[g.below(10)];
            let sb = ALL_STATES[g.below(10)];
            let d = truth.0[sa.low() as usize][m]
                + truth.0[sa.high() as usize][m]
                + truth.1[sb.low() as usize][m]
                + truth.1[sb.high() as usize][m];
            if !first_marker { gl.push(','); }
            first_marker = false;
            gl.push('[');
            for k in 0..5 {
                let distance = (k as f64 - d as f64).abs();
                let mut v = -1.7 * distance - 0.05 * g.float();
                if flat && g.below(4) == 0 && k < 4 {
                    // Make this dosage indistinguishable from the next one up.
                    v = -1.7 * ((k + 1) as f64 - d as f64).abs();
                }
                if k > 0 { gl.push(','); }
                gl.push_str(&format!("{v:?}"));
            }
            gl.push(']');
        }
        gl.push(']');
    }
    gl.push(']');

    let mut rec = String::from("{\"A\":[");
    for m in 0..markers - 1 {
        if m > 0 { rec.push(','); }
        rec.push_str(&format!("{:?}", 0.02 + 0.45 * g.float()));
    }
    rec.push_str("],\"B\":[");
    for m in 0..markers - 1 {
        if m > 0 { rec.push(','); }
        rec.push_str(&format!("{:?}", 0.02 + 0.45 * g.float()));
    }
    rec.push_str("]}");

    let mut dr = String::new();
    dr.push_str("{");
    for (pi, p) in ["A", "B"].iter().enumerate() {
        if pi > 0 { dr.push(','); }
        dr.push_str(&format!("\"{p}\":{{\"allowed\":["));
        let mut allow = vec![false; markers];
        for m in 0..markers {
            allow[m] = g.below(3) > 0;
            if m > 0 { dr.push(','); }
            dr.push_str(if allow[m] { "true" } else { "false" });
        }
        dr.push_str("],\"rate\":[");
        for m in 0..markers {
            if m > 0 { dr.push(','); }
            let rate = if allow[m] { 0.05 + 0.4 * g.float() } else { 0.0 };
            dr.push_str(&format!("{rate:?}"));
        }
        dr.push_str("]}");
    }
    dr.push('}');

    let dosages = format!(
        "{{\"A\":{:?},\"B\":{:?}}}",
        &dose.0[..markers], &dose.1[..markers]
    );

    format!(
        concat!(
            "{{\"format_version\":1,\"markers\":{markers},\"offspring\":{offspring},",
            "\"parent_dosage\":{dosages},\"child_gl\":{gl},",
            "\"recombination\":{rec},\"double_reduction\":{dr}}}"
        ),
        markers = markers,
        offspring = offspring,
        dosages = dosages,
        gl = gl,
        rec = rec,
        dr = dr
    )
}

/// Exhaustive optimum over every admissible parental assignment.
fn brute_force(cross: &Cross, tables: &Tables) -> f64 {
    let chain = Chain::new(cross, tables);
    let mut a_picks = vec![0usize; cross.markers];
    let mut b_picks = vec![0usize; cross.markers];
    let mut best = f64::NEG_INFINITY;
    let mut picks = vec![(0usize, 0usize); cross.markers];

    fn walk(
        cross: &Cross,
        tables: &Tables,
        chain: &Chain,
        depth: usize,
        a_picks: &mut [usize],
        b_picks: &mut [usize],
        picks: &mut [(usize, usize)],
        best: &mut f64,
    ) {
        let markers = cross.markers;
        if depth == markers {
            for m in 0..markers {
                picks[m] = (a_picks[m], b_picks[m]);
            }
            let mut total = 0.0;
            for c in 0..cross.offspring {
                total += chain.best(c, picks);
            }
            if total > *best {
                *best = total;
            }
            return;
        }
        for ia in 0..tables.cols[0][depth].len() {
            a_picks[depth] = ia;
            for ib in 0..tables.cols[1][depth].len() {
                b_picks[depth] = ib;
                walk(cross, tables, chain, depth + 1, a_picks, b_picks, picks, best);
            }
        }
    }

    walk(cross, tables, &chain, 0, &mut a_picks, &mut b_picks, &mut picks, &mut best);
    best
}

fn incumbent_of(cross: &Cross, tables: &Tables, restarts: usize) -> (f64, Vec<(usize, usize)>) {
    let mut rng = Rng::new(0xabcdef);
    let mut local = Local::new(cross, tables);
    let mut best = f64::NEG_INFINITY;
    let mut best_picks: Vec<(usize, usize)> = Vec::new();
    for _ in 0..restarts {
        let mut picks: Vec<(usize, usize)> = (0..cross.markers)
            .map(|m| (rng.below(tables.cols[0][m].len()), rng.below(tables.cols[1][m].len())))
            .collect();
        let v = local.ascend(&mut picks);
        if v > best {
            best = v;
            best_picks = picks;
        }
    }
    (best, best_picks)
}

// -------------------------------------------------------------------------
// Independent checks of a finished answer
// -------------------------------------------------------------------------

fn permutations() -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for a in 0..4u8 {
        for b in 0..4u8 {
            for c in 0..4u8 {
                for d in 0..4u8 {
                    let p = [a, b, c, d];
                    let mut seen = [false; 4];
                    if p.iter().all(|x| !std::mem::replace(&mut seen[*x as usize], true)) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

fn relabel_key(
    answer: &Reconstruction,
    parent: Parent,
    p: &[u8; 4],
) -> (Vec<Vec<u8>>, Vec<u8>, [u8; 4]) {
    let haps = answer.haplotypes(parent);
    let mut rows = vec![Vec::new(); 4];
    for h in 0..4 {
        rows[p[h] as usize] = haps.row(h).to_vec();
    }
    let mut signature = Vec::new();
    for path in &answer.paths {
        for s in path.states(parent) {
            let r = s.remap(p);
            signature.push(r.low());
            signature.push(r.high());
        }
    }
    (rows, signature, *p)
}

fn canonical_defect(answer: &Reconstruction) -> Option<String> {
    let identity = [0u8, 1, 2, 3];
    for parent in Parent::BOTH {
        let own = relabel_key(answer, parent, &identity);
        let rows_sorted = own.0.windows(2).all(|w| w[0] <= w[1]);
        if !rows_sorted {
            return Some(format!("parent {} rows are not in ascending order", parent.label()));
        }
        for p in permutations() {
            if relabel_key(answer, parent, &p) < own {
                return Some(format!(
                    "parent {} has a relabelling that sorts earlier than the answer",
                    parent.label()
                ));
            }
        }
    }
    None
}

fn distinct_key(answer: &Reconstruction) -> Vec<u8> {
    let mut key = Vec::new();
    for parent in Parent::BOTH {
        for h in 0..4 {
            key.extend_from_slice(answer.haplotypes(parent).row(h));
        }
    }
    for path in &answer.paths {
        for parent in Parent::BOTH {
            for s in path.states(parent) {
                key.push(s.low());
                key.push(s.high());
            }
        }
    }
    key
}

/// The rows of a haplotype matrix in ascending order, flattened.
fn sorted_flat(haps: &Haplotypes) -> Vec<u8> {
    let mut rows: Vec<Vec<u8>> = (0..4).map(|h| haps.row(h).to_vec()).collect();
    rows.sort();
    rows.concat()
}

/// Every admissible assignment, with the total it reaches and its sorted rows.
fn all_assignments_with_totals(
    cross: &Cross,
    tables: &Tables,
) -> Vec<(f64, Vec<u8>, Vec<u8>, Vec<(usize, usize)>)> {
    let chain = Chain::new(cross, tables);
    let mut out = Vec::new();
    let mut picks = vec![(0usize, 0usize); cross.markers];
    fn walk(
        cross: &Cross,
        tables: &Tables,
        chain: &Chain,
        depth: usize,
        picks: &mut [(usize, usize)],
        out: &mut Vec<(f64, Vec<u8>, Vec<u8>, Vec<(usize, usize)>)>,
    ) {
        if depth == cross.markers {
            let mut total = 0.0;
            for c in 0..cross.offspring {
                total += chain.best(c, picks);
            }
            let ha = haplotypes_of(tables, picks, Parent::A);
            let hb = haplotypes_of(tables, picks, Parent::B);
            out.push((total, sorted_flat(&ha), sorted_flat(&hb), picks.to_vec()));
            return;
        }
        for ia in 0..tables.cols[0][depth].len() {
            for ib in 0..tables.cols[1][depth].len() {
                picks[depth] = (ia, ib);
                walk(cross, tables, chain, depth + 1, picks, out);
            }
        }
    }
    walk(cross, tables, &chain, 0, &mut picks, &mut out);
    out
}

/// Holds one answer to everything the specification asks of it that can be
/// checked without knowing the answer in advance.
fn verify(cross: &Cross, tables: &Tables) -> Vec<String> {
    let mut problems = Vec::new();
    let answer = solve(cross);
    let total = match score(cross, &answer) {
        Ok(v) => v,
        Err(e) => return vec![format!("the scorer rejects the answer: {e}")],
    };

    let everything = all_assignments_with_totals(cross, tables);
    let optimum = everything.iter().map(|e| e.0).fold(f64::NEG_INFINITY, f64::max);
    if (total - optimum).abs() > 1e-8 {
        problems.push(format!("score {total:.9} is not the optimum {optimum:.9}"));
    }

    // The smallest haplotype key among every assignment that ties.
    let mut best_keys: Option<(Vec<u8>, Vec<u8>)> = None;
    for entry in &everything {
        if entry.0 >= optimum - 1e-8 {
            let key = (entry.1.clone(), entry.2.clone());
            if best_keys.as_ref().map_or(true, |b| &key < b) {
                best_keys = Some(key);
            }
        }
    }
    let mine = (
        sorted_flat(answer.haplotypes(Parent::A)),
        sorted_flat(answer.haplotypes(Parent::B)),
    );
    if let Some(best) = best_keys {
        if best != mine {
            problems.push("a tying assignment has smaller haplotypes than the one reported".into());
        }
    }

    if let Some(defect) = canonical_defect(&answer) {
        problems.push(format!("not canonical: {defect}"));
    }

    // No equally scoring canonical answer may sort before it. Single markers
    // and adjacent pairs, the same scan the diagnostics use.
    let own_key = distinct_key(&answer);
    let mut scan = |variant: &Reconstruction, what: String| {
        let Ok(other) = score(cross, variant) else { return };
        if (other - total).abs() > 1e-8 || canonical_defect(variant).is_some() {
            return;
        }
        if distinct_key(variant) < own_key {
            problems.push(format!("an equally scoring canonical answer sorts earlier: {what}"));
        }
    };
    for c in 0..cross.offspring {
        for parent in Parent::BOTH {
            for m in 0..cross.markers {
                for alternative in ALL_STATES {
                    let mut variant = answer.clone();
                    let states = match parent {
                        Parent::A => &mut variant.paths[c].from_a,
                        Parent::B => &mut variant.paths[c].from_b,
                    };
                    if states[m] == alternative {
                        continue;
                    }
                    states[m] = alternative;
                    scan(&variant, format!("progeny {c} parent {} marker {m}", parent.label()));
                }
            }
            for m in 0..cross.markers - 1 {
                for first in ALL_STATES {
                    for second in ALL_STATES {
                        let mut variant = answer.clone();
                        let states = match parent {
                            Parent::A => &mut variant.paths[c].from_a,
                            Parent::B => &mut variant.paths[c].from_b,
                        };
                        if states[m] == first && states[m + 1] == second {
                            continue;
                        }
                        states[m] = first;
                        states[m + 1] = second;
                        scan(
                            &variant,
                            format!("progeny {c} parent {} markers {m}-{}", parent.label(), m + 1),
                        );
                    }
                }
            }
        }
    }

    // Two runs must agree.
    if solve(cross) != answer {
        problems.push("two runs returned different answers".into());
    }
    problems
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).cloned().unwrap_or_default();
    if mode == "verify" {
        let cases: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(10);
        let markers: usize = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(12);
        let offspring: usize = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(8);
        let free: usize = args.get(5).map(|s| s.parse().unwrap()).unwrap_or(2);
        let flat: bool = args.get(6).map(|s| s == "flat").unwrap_or(false);
        for seed in 0..cases {
            let text = random_cross(seed as u64 + 1, markers, offspring, free, flat);
            std::fs::write("/tmp/gen_cross.json", &text).unwrap();
            let cross = parse_cross(&text).expect("generated cross must be valid");
            let tables = Tables::build(&cross);
            let problems = verify(&cross, &tables);
            if problems.is_empty() {
                println!("seed {seed}: ok");
            } else {
                std::fs::write("/tmp/failing_cross.json", &text).unwrap();
                for p in problems {
                    println!("seed {seed}: {p}");
                }
                println!("  wrote /tmp/failing_cross.json");
                break;
            }
        }
        return;
    }
    if mode == "make" {
        // Writes a generated cross to a path, for timing runs.
        let seed: u64 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(1);
        let markers: usize = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(28);
        let offspring: usize = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(32);
        let free: usize = args.get(5).map(|s| s.parse().unwrap()).unwrap_or(28);
        let flat: bool = args.get(6).map(|s| s == "flat").unwrap_or(false);
        let out = args.get(7).cloned().unwrap_or_else(|| "/tmp/large_cross.json".to_string());
        let text = random_cross(seed, markers, offspring, free, flat);
        std::fs::write(&out, &text).unwrap();
        println!("wrote {out} ({markers} markers, {offspring} progeny)");
        return;
    }
    if mode == "profile" {
        let path = args.get(2).cloned().unwrap_or_else(|| "/tmp/large_cross.json".to_string());
        let restarts: usize = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(8);
        let text = std::fs::read_to_string(&path).unwrap();
        let cross = parse_cross(&text).unwrap();
        let tables = Tables::build(&cross);
        let bounds = SuffixBounds::build(&cross, &tables);
        let mut rng = Rng::new(0xabcdef);
        let mut local = Local::new(&cross, &tables);
        let mut floor = f64::NEG_INFINITY;
        let t0 = std::time::Instant::now();
        for _ in 0..restarts {
            let mut picks: Vec<(usize, usize)> = (0..cross.markers)
                .map(|m| (rng.below(tables.cols[0][m].len()), rng.below(tables.cols[1][m].len())))
                .collect();
            let v = local.ascend(&mut picks);
            if v > floor {
                floor = v;
            }
        }
        println!("local: {restarts} restarts in {:?}, floor {floor:.6}", t0.elapsed());
        println!("root bound {:.6}, gap {:.6}", bounds.root(), bounds.root() - floor);
        let t1 = std::time::Instant::now();
        let mut search = Search::new(&cross, &tables, &bounds);
        search.progress = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(0);
        let cands = search.run(floor);
        println!(
            "bb: {:?} nodes={} leaves={} cands={} best={:.6} root={:.6}",
            t1.elapsed(),
            search.nodes,
            search.leaves,
            cands.len(),
            search.best_score(),
            bounds.root()
        );
        return;
    }
    if mode == "time" {
        // Times `solve` on a cross file, without any enumeration.
        let path = args.get(2).cloned().unwrap_or_else(|| "/tmp/large_cross.json".to_string());
        let text = std::fs::read_to_string(&path).unwrap();
        let cross = parse_cross(&text).unwrap();
        let t0 = std::time::Instant::now();
        let answer = solve(&cross);
        let elapsed = t0.elapsed();
        let total = score(&cross, &answer).unwrap();
        println!("solve took {elapsed:?}, score {total:.9}");
        return;
    }
    if mode == "random" {
        let cases: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(20);
        let markers: usize = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(12);
        let offspring: usize = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(8);
        let free: usize = args.get(5).map(|s| s.parse().unwrap()).unwrap_or(2);
        let flat: bool = args.get(6).map(|s| s == "flat").unwrap_or(false);
        let mut worst = 0.0f64;
        for seed in 0..cases {
            let text = random_cross(seed as u64 + 1, markers, offspring, free, flat);
            std::fs::write("/tmp/gen_cross.json", &text).unwrap();
            let cross = parse_cross(&text).expect("generated cross must be valid");
            let tables = Tables::build(&cross);
            let bounds = SuffixBounds::build(&cross, &tables);
            let (inc, _picks) = incumbent_of(&cross, &tables, 12);
            let mut search = Search::new(&cross, &tables, &bounds);
            let t0 = std::time::Instant::now();
            let cands = search.run(inc);
            let found = search.best_score();
            let brute = brute_force(&cross, &tables);
            let ok = (found - brute).abs() < 1e-8;
            println!(
                "seed {seed} len={}: brute={brute:.9} bb={found:.9} ok={ok} nodes={} leaves={} cands={} t={:?}",
                text.len(), search.nodes, search.leaves, cands.len(), t0.elapsed()
            );
            worst = worst.max((found - brute).abs());
            if !ok {
                std::fs::write("/tmp/failing_cross.json", &text).unwrap();
                println!("  wrote /tmp/failing_cross.json");
                break;
            }
        }
        println!("worst discrepancy {worst:e}");
        return;
    }

    // Otherwise: run on a file and report.
    let path = args.get(1).cloned().unwrap_or_else(|| "/app/cross.json".to_string());
    let text = std::fs::read_to_string(&path).unwrap();
    let cross = parse_cross(&text).unwrap();
    let tables = Tables::build(&cross);
    let bounds = SuffixBounds::build(&cross, &tables);
    let (inc, picks) = incumbent_of(&cross, &tables, 60);
    println!("incumbent {inc:.9}");
    let reduced = reduce(&cross, &tables, inc);
    let bounds = SuffixBounds::build(&cross, &tables, &reduced.ub);
    let mut search = Search::new(&cross, &tables, &bounds, &reduced);
    let t0 = std::time::Instant::now();
    let cands = search.run(inc);
    println!(
        "bb best={:.9} nodes={} leaves={} cands={} t={:?}",
        search.best_score(),
        search.nodes,
        search.leaves,
        cands.len(),
        t0.elapsed()
    );
    let _ = picks;
    let answer = solve(&cross);
    println!("solve score = {:.9}", score(&cross, &answer).unwrap());
    let _: Option<State> = None;
}
