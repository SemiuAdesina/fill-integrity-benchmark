// Scratch verification: independent exhaustive enumeration, unpruned
// comparison, and timing on the largest inputs the format allows.
// Deleted before delivery.

use polyploid_phase::cross_input::{parse_cross, Cross};
use polyploid_phase::cross_model::{
    Haplotypes, Parent, Reconstruction, State, ALL_STATES, HOMOLOGS,
};
use polyploid_phase::scoring::{
    double_reduction_term, score, scores_equal, transition_term,
};
use polyploid_phase::solver::canon::canonicalize;
use polyploid_phase::solver::solve;

// --------------------------------------------------------------------------
// Instance construction
// --------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % (n as u64)) as usize
    }
    fn float(&mut self) -> f64 {
        (self.next() % 100000) as f64 / 100000.0
    }
}

fn json_numbers(items: &[f64]) -> String {
    let parts: Vec<String> = items.iter().map(|x| format!("{x:?}")).collect();
    format!("[{}]", parts.join(","))
}

fn json_ints(items: &[u8]) -> String {
    let parts: Vec<String> = items.iter().map(|x| x.to_string()).collect();
    format!("[{}]", parts.join(","))
}

fn json_bools(items: &[bool]) -> String {
    let parts: Vec<String> = items.iter().map(|x| x.to_string()).collect();
    format!("[{}]", parts.join(","))
}

#[allow(clippy::too_many_arguments)]
fn build_cross(
    dosage_a: &[u8],
    dosage_b: &[u8],
    gl: &[Vec<[f64; 5]>],
    rec_a: &[f64],
    rec_b: &[f64],
    allowed_a: &[bool],
    allowed_b: &[bool],
    rate_a: &[f64],
    rate_b: &[f64],
) -> Cross {
    let gl_text: Vec<String> = gl
        .iter()
        .map(|progeny| {
            let rows: Vec<String> = progeny.iter().map(|r| json_numbers(r)).collect();
            format!("[{}]", rows.join(","))
        })
        .collect();
    let text = format!(
        concat!(
            "{{\"format_version\":1,\"markers\":{},\"offspring\":{},",
            "\"parent_dosage\":{{\"A\":{},\"B\":{}}},",
            "\"child_gl\":[{}],",
            "\"recombination\":{{\"A\":{},\"B\":{}}},",
            "\"double_reduction\":{{",
            "\"A\":{{\"allowed\":{},\"rate\":{}}},",
            "\"B\":{{\"allowed\":{},\"rate\":{}}}}}}}"
        ),
        dosage_a.len(),
        gl.len(),
        json_ints(dosage_a),
        json_ints(dosage_b),
        gl_text.join(","),
        json_numbers(rec_a),
        json_numbers(rec_b),
        json_bools(allowed_a),
        json_numbers(rate_a),
        json_bools(allowed_b),
        json_numbers(rate_b),
    );
    parse_cross(&text).expect("scratch cross must be valid")
}

fn peaked(truth: u8, slope: f64, tilt: f64) -> [f64; 5] {
    let mut out = [0.0f64; 5];
    for (d, slot) in out.iter_mut().enumerate() {
        let distance = (d as f64 - f64::from(truth)).abs();
        *slot = -slope * distance - tilt * d as f64;
    }
    out
}

fn admissible_columns(dosage: u8) -> Vec<[u8; HOMOLOGS]> {
    let mut out = Vec::new();
    for bits in 0u8..16 {
        if bits.count_ones() as u8 == dosage {
            out.push([bits & 1, (bits >> 1) & 1, (bits >> 2) & 1, (bits >> 3) & 1]);
        }
    }
    out
}

fn haps_from_columns(columns: &[[u8; HOMOLOGS]]) -> Haplotypes {
    let mut haps = Haplotypes::zeros(columns.len());
    for (m, column) in columns.iter().enumerate() {
        for h in 0..HOMOLOGS {
            haps.set(h, m, column[h]);
        }
    }
    haps
}

// --------------------------------------------------------------------------
// Independent exhaustive optimum, written against the public scorer's terms
// --------------------------------------------------------------------------

fn best_path_total(cross: &Cross, hap_a: &Haplotypes, hap_b: &Haplotypes, c: usize) -> f64 {
    let joint: Vec<(State, State)> = ALL_STATES
        .iter()
        .flat_map(|a| ALL_STATES.iter().map(move |b| (*a, *b)))
        .collect();
    let legal = |sa: State, sb: State, m: usize| {
        (!sa.is_double_reduction() || cross.params(Parent::A).dr_allowed[m])
            && (!sb.is_double_reduction() || cross.params(Parent::B).dr_allowed[m])
    };
    let node = |sa: State, sb: State, m: usize| {
        let dosage = hap_a.row(sa.low() as usize)[m]
            + hap_a.row(sa.high() as usize)[m]
            + hap_b.row(sb.low() as usize)[m]
            + hap_b.row(sb.high() as usize)[m];
        cross.child_gl[c][m][usize::from(dosage)]
            + double_reduction_term(cross, Parent::A, sa, m)
            + double_reduction_term(cross, Parent::B, sb, m)
    };
    let mut best: Vec<f64> = joint
        .iter()
        .map(|(sa, sb)| if legal(*sa, *sb, 0) { node(*sa, *sb, 0) } else { f64::NEG_INFINITY })
        .collect();
    for m in 1..cross.markers {
        let mut next = vec![f64::NEG_INFINITY; joint.len()];
        let mut tbl = [[0.0f64; 10]; 10];
        let mut tal = [[0.0f64; 10]; 10];
        for x in 0..10 {
            for y in 0..10 {
                tbl[x][y] = transition_term(cross, Parent::B, ALL_STATES[x], ALL_STATES[y], m - 1);
                tal[x][y] = transition_term(cross, Parent::A, ALL_STATES[x], ALL_STATES[y], m - 1);
            }
        }
        let mut partial = [f64::NEG_INFINITY; 100];
        for to_b in 0..10 {
            for from_a in 0..10 {
                let mut top = f64::NEG_INFINITY;
                for from_b in 0..10 {
                    let v = best[from_a * 10 + from_b] + tbl[from_b][to_b];
                    if v > top {
                        top = v;
                    }
                }
                partial[to_b * 10 + from_a] = top;
            }
        }
        for (i, (sa, sb)) in joint.iter().enumerate() {
            if !legal(*sa, *sb, m) {
                continue;
            }
            let ia = sa.canonical_index();
            let ib = sb.canonical_index();
            let mut top = f64::NEG_INFINITY;
            for from_a in 0..10 {
                let v = partial[ib * 10 + from_a] + tal[from_a][ia];
                if v > top {
                    top = v;
                }
            }
            next[i] = top + node(*sa, *sb, m);
        }
        best = next;
    }
    best.into_iter().fold(f64::NEG_INFINITY, f64::max)
}

fn all_assignments(dosage: &[u8], markers: usize) -> Vec<Haplotypes> {
    let per_marker: Vec<Vec<[u8; HOMOLOGS]>> =
        dosage.iter().map(|d| admissible_columns(*d)).collect();
    let mut out = Vec::new();
    let mut current = vec![[0u8; HOMOLOGS]; markers];
    fn walk(
        m: usize,
        per_marker: &[Vec<[u8; HOMOLOGS]>],
        current: &mut Vec<[u8; HOMOLOGS]>,
        out: &mut Vec<Haplotypes>,
    ) {
        if m == per_marker.len() {
            out.push(haps_from_columns(current));
            return;
        }
        for column in &per_marker[m] {
            current[m] = *column;
            walk(m + 1, per_marker, current, out);
        }
    }
    walk(0, &per_marker, &mut current, &mut out);
    out
}

fn exhaustive_optimum(cross: &Cross) -> f64 {
    let a = all_assignments(&cross.params(Parent::A).dosage, cross.markers);
    let b = all_assignments(&cross.params(Parent::B).dosage, cross.markers);
    let mut best = f64::NEG_INFINITY;
    for hap_a in &a {
        for hap_b in &b {
            let total: f64 = (0..cross.offspring)
                .map(|c| best_path_total(cross, hap_a, hap_b, c))
                .sum();
            if total > best {
                best = total;
            }
        }
    }
    best
}

// --------------------------------------------------------------------------
// Generators
// --------------------------------------------------------------------------

struct Instance {
    cross: Cross,
    truth: Reconstruction,
}

fn random_instance(
    rng: &mut Rng,
    markers: usize,
    offspring: usize,
    free: usize,
    slope: f64,
    planted: bool,
) -> Instance {
    let mut dosage_a = vec![0u8; markers];
    let mut dosage_b = vec![0u8; markers];
    let mut slots: Vec<usize> = (0..markers).collect();
    for _ in 0..free {
        let index = rng.below(slots.len());
        dosage_a[slots.remove(index)] = 1 + rng.below(3) as u8;
    }
    let mut slots: Vec<usize> = (0..markers).collect();
    for _ in 0..free {
        let index = rng.below(slots.len());
        dosage_b[slots.remove(index)] = 1 + rng.below(3) as u8;
    }

    let mut columns_a = Vec::new();
    let mut columns_b = Vec::new();
    for m in 0..markers {
        let options = admissible_columns(dosage_a[m]);
        columns_a.push(options[rng.below(options.len())]);
        let options = admissible_columns(dosage_b[m]);
        columns_b.push(options[rng.below(options.len())]);
    }
    let hap_a = haps_from_columns(&columns_a);
    let hap_b = haps_from_columns(&columns_b);

    let mut paths = Vec::new();
    let mut gl = Vec::new();
    for _ in 0..offspring {
        let mut from_a = Vec::new();
        let mut from_b = Vec::new();
        let mut rows = Vec::new();
        for m in 0..markers {
            let sa = loop {
                let s = ALL_STATES[rng.below(ALL_STATES.len())];
                if !s.is_double_reduction() {
                    break s;
                }
            };
            let sb = ALL_STATES[rng.below(ALL_STATES.len())];
            from_a.push(sa);
            from_b.push(sb);
            let truth = hap_a.row(sa.low() as usize)[m]
                + hap_a.row(sa.high() as usize)[m]
                + hap_b.row(sb.low() as usize)[m]
                + hap_b.row(sb.high() as usize)[m];
            let mut row = peaked(truth, slope, 0.01);
            if !planted {
                for slot in row.iter_mut() {
                    *slot = -3.0 * rng.float();
                }
            }
            rows.push(row);
        }
        paths.push(polyploid_phase::cross_model::ProgenyPath { from_a, from_b });
        gl.push(rows);
    }

    let rec_a: Vec<f64> = (0..markers - 1).map(|_| 0.05 + 0.2 * rng.float()).collect();
    let rec_b: Vec<f64> = (0..markers - 1).map(|_| 0.05 + 0.2 * rng.float()).collect();
    let allowed_a: Vec<bool> = (0..markers).map(|_| rng.below(2) == 0).collect();
    let allowed_b: Vec<bool> = (0..markers).map(|_| rng.below(2) == 0).collect();
    let rate_a: Vec<f64> = allowed_a
        .iter()
        .map(|a| if *a { 0.1 + 0.3 * rng.float() } else { 0.0 })
        .collect();
    let rate_b: Vec<f64> = allowed_b
        .iter()
        .map(|a| if *a { 0.1 + 0.3 * rng.float() } else { 0.0 })
        .collect();

    let cross = build_cross(
        &dosage_a, &dosage_b, &gl, &rec_a, &rec_b,
        &allowed_a, &allowed_b, &rate_a, &rate_b,
    );
    Instance {
        cross,
        truth: Reconstruction { haplotypes_a: hap_a, haplotypes_b: hap_b, paths },
    }
}

// --------------------------------------------------------------------------
// Checks
// --------------------------------------------------------------------------

#[test]
fn matches_independent_enumeration() {
    let mut rng = Rng(0xc0ffee);
    for case in 0..12 {
        let instance = random_instance(&mut rng, 12, 8, 2, 1.0 + rng.float(), true);
        let answer = solve(&instance.cross);
        let total = score(&instance.cross, &answer).expect("scorable");
        let optimum = exhaustive_optimum(&instance.cross);
        assert!(
            scores_equal(total, optimum),
            "case {case}: solver {total:.9} enumeration {optimum:.9}"
        );
        assert_eq!(canonicalize(&answer), answer, "case {case}: not canonical");
        assert!(total >= score(&instance.cross, &instance.truth).unwrap() - 1e-9);
    }
}

#[test]
fn matches_unpruned_enumeration() {
    let mut rng = Rng(0xfeed);
    for case in 0..8 {
        let instance = random_instance(&mut rng, 12, 8, 4, 1.0 + rng.float(), true);
        let answer = solve(&instance.cross);
        let total = score(&instance.cross, &answer).expect("scorable");

        // Enumerate every canonical assignment with pruning switched off, which
        // is what `collecting` with no floor does.
        let core = polyploid_phase::solver::core::Core::new(&instance.cross);
        let mut search = polyploid_phase::solver::search::Search::new(&core);
        let all = search.run(f64::NEG_INFINITY, true);
        let mut reference = f64::NEG_INFINITY;
        for candidate in &all {
            if candidate.score > reference {
                reference = candidate.score;
            }
        }
        assert!(
            scores_equal(total, reference),
            "case {case}: branch and bound {total:.9} unpruned {reference:.9} ({})",
            all.len()
        );
    }
}

#[test]
fn largest_inputs_finish() {
    let mut rng = Rng(0x1234abcd);
    for (index, (markers, offspring)) in [(28usize, 32usize), (28, 16), (20, 32), (16, 8)]
        .iter()
        .enumerate()
    {
        for planted in [true, false] {
            let instance = random_instance(&mut rng, *markers, *offspring, *markers, 1.5, planted);
            let start = std::time::Instant::now();
            let answer = solve(&instance.cross);
            let elapsed = start.elapsed();
            let total = score(&instance.cross, &answer).expect("scorable");
            assert_eq!(canonicalize(&answer), answer, "not canonical");
            println!(
                "m={markers} n={offspring} planted={planted} score={total:.4} time={:?}",
                elapsed
            );
            assert!(elapsed.as_secs() < 60, "case {index} planted={planted} took {elapsed:?}");
        }
    }
}
