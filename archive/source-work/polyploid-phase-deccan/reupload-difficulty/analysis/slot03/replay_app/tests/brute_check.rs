//! Brute force validation of the search on crosses small enough to enumerate.
//!
//! Each generated cross pins most markers to dosage 0 or 4, so the admissible
//! parental assignments are few and the optimum can be found with no search at
//! all. The check then holds `solve` to that optimum, to the canonical form, and
//! to the documented representative among equally scoring answers.
//!
//! Run with:
//!
//!     cargo test --offline --locked --release --test brute_check

use polyploid_phase::cross_input::{parse_cross, Cross};
use polyploid_phase::cross_model::{Haplotypes, Parent, Reconstruction, State, ALL_STATES, HOMOLOGS};
use polyploid_phase::solver::canon::{canonical_defect, canonicalise, distinct_key, remap_table, row_permutations};
use polyploid_phase::solver::model::Model;
use polyploid_phase::solver::viterbi::{best_total, haplotypes_from_cols, lexicographic_path};
use polyploid_phase::solver::solve;
use polyploid_phase::scoring::{score, scores_equal};

const M: usize = 12;
const N: usize = 8;

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x5851F42D4C957F2D)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A generated cross. `free` markers carry dosage 1, 2 or 3 and the rest 0 or 4.
/// With `ties` the likelihoods are quantised so that distinct dosages often tie.
fn make_json(seed: u64, free: usize, ties: bool) -> String {
    let mut rng = Rng::new(seed);
    let mut dos_a = [0u8; M];
    let mut dos_b = [0u8; M];
    for slot in dos_a.iter_mut() {
        *slot = if rng.below(2) == 0 { 0 } else { 4 };
    }
    for slot in dos_b.iter_mut() {
        *slot = if rng.below(2) == 0 { 0 } else { 4 };
    }
    let mut positions: Vec<usize> = (0..M).collect();
    for _ in 0..free {
        let i = rng.below(positions.len());
        let m = positions.remove(i);
        dos_a[m] = 1 + rng.below(3) as u8;
        let i = rng.below(positions.len());
        let m = positions.remove(i);
        dos_b[m] = 1 + rng.below(3) as u8;
    }

    // Planted truth: one admissible column per marker for each parent.
    let mut ca = [0u8; M];
    let mut cb = [0u8; M];
    for m in 0..M {
        ca[m] = pick_column(&mut rng, dos_a[m]);
        cb[m] = pick_column(&mut rng, dos_b[m]);
    }
    let bits = |mask: u8, h: usize| (mask >> h) & 1;
    let dosage = |sa: State, sb: State, m: usize| -> usize {
        (bits(ca[m], sa.low() as usize)
            + bits(ca[m], sa.high() as usize)
            + bits(cb[m], sb.low() as usize)
            + bits(cb[m], sb.high() as usize)) as usize
    };

    let mut gl = String::new();
    for c in 0..N {
        let mut sa = ALL_STATES[rng.below(10)];
        let mut sb = ALL_STATES[rng.below(10)];
        if c == N - 1 {
            sa = State::new(0, 0).unwrap();
            sb = State::new(0, 0).unwrap();
        }
        gl.push('[');
        for m in 0..M {
            let truth = dosage(sa, sb, m);
            gl.push('[');
            for d in 0..5 {
                let distance = if d > truth { d - truth } else { truth - d };
                let mut v = -1.7 * distance as f64;
                if ties {
                    v = (v * 2.0).round() / 2.0;
                } else {
                    v -= 0.01 * d as f64;
                }
                gl.push_str(&format!("{v:.4}"));
                if d < 4 {
                    gl.push(',');
                }
            }
            gl.push(']');
            if m + 1 < M {
                gl.push(',');
            }
            if rng.below(100) < 10 {
                sa = ALL_STATES[rng.below(10)];
            }
            if rng.below(100) < 10 {
                sb = ALL_STATES[rng.below(10)];
            }
        }
        gl.push(']');
        if c + 1 < N {
            gl.push(',');
        }
    }

    let list = |items: &[u8]| -> String {
        items.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")
    };
    let rec = |rng: &mut Rng| -> String {
        (0..M - 1)
            .map(|_| format!("{:.4}", 0.05 + 0.2 * (rng.below(1000) as f64 / 1000.0)))
            .collect::<Vec<_>>()
            .join(",")
    };
    let rec_a = rec(&mut rng);
    let rec_b = rec(&mut rng);
    let dr = |rng: &mut Rng| -> (String, String) {
        let mut allowed = Vec::new();
        let mut rate = Vec::new();
        for _ in 0..M {
            if rng.below(2) == 0 {
                allowed.push("true".to_string());
                rate.push(format!("{:.3}", 0.05 + 0.3 * (rng.below(1000) as f64 / 1000.0)));
            } else {
                allowed.push("false".to_string());
                rate.push("0.0".to_string());
            }
        }
        (allowed.join(","), rate.join(","))
    };
    let (aa, ra) = dr(&mut rng);
    let (ab, rb) = dr(&mut rng);

    format!(
        concat!(
            "{{\"format_version\":1,\"markers\":{},\"offspring\":{},",
            "\"parent_dosage\":{{\"A\":[{}],\"B\":[{}]}},",
            "\"child_gl\":[{}],",
            "\"recombination\":{{\"A\":[{}],\"B\":[{}]}},",
            "\"double_reduction\":{{\"A\":{{\"allowed\":[{}],\"rate\":[{}]}},",
            "\"B\":{{\"allowed\":[{}],\"rate\":[{}]}}}}}}"
        ),
        M, N, list(&dos_a), list(&dos_b), gl, rec_a, rec_b, aa, ra, ab, rb
    )
}

fn pick_column(rng: &mut Rng, dosage: u8) -> u8 {
    let mut options: Vec<u8> = (0u8..16).filter(|m| m.count_ones() as u8 == dosage).collect();
    let i = rng.below(options.len());
    options.swap_remove(i)
}

/// Maximum over every admissible haplotype pair, found by enumeration.
fn exhaustive(model: &Model) -> (f64, Vec<(Vec<usize>, Vec<usize>)>) {
    let seq_a: Vec<Vec<usize>> = column_sequences_raw(model, Parent::A);
    let seq_b: Vec<Vec<usize>> = column_sequences_raw(model, Parent::B);
    let mut best = f64::NEG_INFINITY;
    let mut optima = Vec::new();
    for a in &seq_a {
        for b in &seq_b {
            let cols: Vec<(usize, usize)> =
                (0..model.m).map(|mm| (a[mm], b[mm])).collect();
            let total: f64 = (0..model.n).map(|c| best_total(model, c, &cols)).sum();
            if total > best + 1e-9 {
                best = total;
                optima.clear();
                optima.push((a.clone(), b.clone()));
            } else if total > best - 1e-8 {
                optima.push((a.clone(), b.clone()));
            }
        }
    }
    (best, optima)
}

fn column_sequences_raw(model: &Model, parent: Parent) -> Vec<Vec<usize>> {
    let p = parent.index();
    let mut per_marker: Vec<Vec<usize>> = Vec::new();
    for mm in 0..model.m {
        per_marker.push((0..model.cols[p][mm].len()).collect());
    }
    let mut out = Vec::new();
    let mut current = vec![0usize; model.m];
    fn walk(
        mm: usize,
        per_marker: &[Vec<usize>],
        current: &mut [usize],
        out: &mut Vec<Vec<usize>>,
    ) {
        if mm == per_marker.len() {
            out.push(current.to_vec());
            return;
        }
        for &i in &per_marker[mm] {
            current[mm] = i;
            walk(mm + 1, per_marker, current, out);
        }
    }
    walk(0, &per_marker, &mut current, &mut out);
    out
}

/// Flat row major key of a haplotype matrix with its rows sorted.
fn sorted_key_of(haps: &Haplotypes) -> Vec<u8> {
    let mut rows: Vec<Vec<u8>> = (0..HOMOLOGS).map(|h| haps.row(h).to_vec()).collect();
    rows.sort();
    rows.into_iter().flatten().collect()
}

/// Flat row major key of a sorted haplotype matrix.
fn sorted_matrix_key(model: &Model, parent: Parent, cols: &[(usize, usize)]) -> Vec<u8> {
    let haps = haplotypes_from_cols(model, parent, cols);
    let mut rows: Vec<Vec<u8>> = (0..HOMOLOGS).map(|h| haps.row(h).to_vec()).collect();
    rows.sort();
    rows.into_iter().flatten().collect()
}

/// Builds a reconstruction for a column sequence with the smallest path
/// signature among the optimal ones.
fn reconstruct(model: &Model, cols: &[(usize, usize)]) -> Reconstruction {
    let hap_a = haplotypes_from_cols(model, Parent::A, cols);
    let hap_b = haplotypes_from_cols(model, Parent::B, cols);
    let mut best: Option<(Vec<u8>, Reconstruction)> = None;
    for pa in row_permutations(&hap_a) {
        for pb in row_permutations(&hap_b) {
            let ra = remap_table(&pa);
            let rb = remap_table(&pb);
            let mut paths = Vec::with_capacity(model.n);
            for c in 0..model.n {
                paths.push(lexicographic_path(model, c, cols, &ra, &rb));
            }
            let candidate = Reconstruction {
                haplotypes_a: hap_a.clone(),
                haplotypes_b: hap_b.clone(),
                paths,
            };
            let canonical = canonicalise(&candidate);
            let key = distinct_key(&canonical);
            if best.as_ref().map_or(true, |(k, _)| key < *k) {
                best = Some((key, canonical));
            }
        }
    }
    best.expect("there is always a candidate").1
}

fn check(seed: u64, free: usize, ties: bool) {
    let text = make_json(seed, free, ties);
    let cross: Cross = parse_cross(&text).expect("generated cross must be valid");
    let model = Model::build(&cross);

    let (optimum, optima) = exhaustive(&model);
    let answer = solve(&cross);
    let total = score(&cross, &answer).expect("the answer must be scoreable");

    assert!(
        scores_equal(total, optimum) || total > optimum,
        "seed {seed}: the search reports {total:.10} where enumeration finds {optimum:.10}"
    );
    assert!(
        total >= optimum - 1e-8,
        "seed {seed}: the search reports {total:.10}, below the enumerated {optimum:.10}"
    );

    if let Some(defect) = canonical_defect(&answer) {
        panic!("seed {seed}: {defect}");
    }

    // The haplotype pair must be the smallest among the enumerated optima.
    let own_a = sorted_key_of(answer.haplotypes(Parent::A));
    let own_b = sorted_key_of(answer.haplotypes(Parent::B));
    for (a, b) in &optima {
        let cols: Vec<(usize, usize)> = (0..model.m).map(|mm| (a[mm], b[mm])).collect();
        let ka = sorted_matrix_key(&model, Parent::A, &cols);
        let kb = sorted_matrix_key(&model, Parent::B, &cols);
        assert!(
            (own_a.clone(), own_b.clone()) <= (ka, kb),
            "seed {seed}: an enumerated optimum sorts before the reported haplotypes"
        );
    }

    // No equally scoring single state change may sort earlier.
    let own_key = distinct_key(&answer);
    for c in 0..cross.offspring {
        for parent in Parent::BOTH {
            for mm in 0..cross.markers {
                for alternative in ALL_STATES {
                    let mut variant = answer.clone();
                    let states = match parent {
                        Parent::A => &mut variant.paths[c].from_a,
                        Parent::B => &mut variant.paths[c].from_b,
                    };
                    if states[mm] == alternative {
                        continue;
                    }
                    states[mm] = alternative;
                    let Ok(other) = score(&cross, &variant) else { continue };
                    if !scores_equal(other, total) || canonical_defect(&variant).is_some() {
                        continue;
                    }
                    assert!(
                        distinct_key(&variant) >= own_key,
                        "seed {seed}: progeny {c} parent {} marker {mm}: an equally scoring \
                         canonical answer sorts earlier",
                        parent.label()
                    );
                }
            }
        }
    }
}

#[test]
fn matches_enumeration_on_random_crosses() {
    for seed in 0..12u64 {
        check(seed, 2, false);
    }
}

#[test]
fn matches_enumeration_with_ties() {
    for seed in 100..112u64 {
        check(seed, 2, true);
    }
}
