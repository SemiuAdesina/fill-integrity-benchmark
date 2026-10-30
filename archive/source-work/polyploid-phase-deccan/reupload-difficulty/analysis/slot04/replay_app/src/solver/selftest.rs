
//! Randomised cross checks of the search against full enumeration.
//!
//! A cross whose dosage is mostly zero or four has few admissible parental
//! assignments, so the optimum can be found with no search at all. Every check
//! here builds such a cross, compares the branch and bound against
//! enumeration, and then checks the answer the whole pipeline reports against
//! the frozen scorer and against canonical form.

#[cfg(test)]
mod tests {
    use crate::cross_input::parse_cross;
    use crate::cross_model::Parent;
    use crate::result_output::{decode, encode};
    use crate::scoring::{score, scores_equal};
    use crate::solver::canonical::{canonicalize, distinct_key};
    use crate::solver::core::Tables;
    use crate::solver::exact::Branch;
    use crate::solver::search::{self, Rng};
    use crate::solver::solve;

    /// Builds a cross document with `free` markers per parent carrying an
    /// informative dosage and the rest pinned at zero or four.
    fn random_cross(rng: &mut Rng, markers: usize, offspring: usize, free: usize) -> String {
        let mut dosage_a = vec![0u8; markers];
        let mut dosage_b = vec![0u8; markers];
        for _ in 0..free {
            loop {
                let m = rng.below(markers);
                if dosage_a[m] == 0 {
                    dosage_a[m] = 1 + rng.below(3) as u8;
                    break;
                }
            }
            loop {
                let m = rng.below(markers);
                if dosage_b[m] == 0 {
                    dosage_b[m] = 1 + rng.below(3) as u8;
                    break;
                }
            }
        }
        for slot in dosage_a.iter_mut() {
            if *slot == 0 {
                *slot = if rng.below(2) == 0 { 0 } else { 4 };
            }
        }
        for slot in dosage_b.iter_mut() {
            if *slot == 0 {
                *slot = if rng.below(2) == 0 { 0 } else { 4 };
            }
        }

        let mut gl = String::new();
        for c in 0..offspring {
            if c > 0 {
                gl.push(',');
            }
            gl.push('[');
            for m in 0..markers {
                if m > 0 {
                    gl.push(',');
                }
                let peak = rng.below(5);
                let slope = 0.8 + (rng.below(200) as f64) / 100.0;
                gl.push('[');
                for d in 0..5 {
                    if d > 0 {
                        gl.push(',');
                    }
                    let distance = (d as i64 - peak as i64).abs() as f64;
                    let jitter = (rng.below(100) as f64) / 1000.0;
                    gl.push_str(&format!("{:.6}", -slope * distance - jitter));
                }
                gl.push(']');
            }
            gl.push(']');
        }

        let list = |items: &[u8]| -> String {
            let parts: Vec<String> = items.iter().map(|x| x.to_string()).collect();
            format!("[{}]", parts.join(","))
        };
        fn floats(rng: &mut Rng, n: usize, lo: f64, hi: f64) -> String {
            let parts: Vec<String> = (0..n)
                .map(|_| format!("{:.6}", lo + (hi - lo) * (rng.below(1000) as f64) / 1000.0))
                .collect();
            format!("[{}]", parts.join(","))
        }
        let rec_a = floats(rng, markers - 1, 0.01, 0.4);
        let rec_b = floats(rng, markers - 1, 0.01, 0.4);
        fn dr_pair(rng: &mut Rng, n: usize) -> (String, String) {
            let mut allowed = Vec::new();
            let mut rate = Vec::new();
            for _ in 0..n {
                if rng.below(3) == 0 {
                    allowed.push("true".to_string());
                    rate.push(format!("{:.6}", 0.05 + 0.4 * (rng.below(1000) as f64) / 1000.0));
                } else {
                    allowed.push("false".to_string());
                    rate.push("0.0".to_string());
                }
            }
            (format!("[{}]", allowed.join(",")), format!("[{}]", rate.join(",")))
        }
        let (allowed_a, rate_a) = dr_pair(rng, markers);
        let (allowed_b, rate_b) = dr_pair(rng, markers);

        format!(
            concat!(
                "{{\"format_version\":1,\"markers\":{},\"offspring\":{},",
                "\"parent_dosage\":{{\"A\":{},\"B\":{}}},",
                "\"child_gl\":[{}],",
                "\"recombination\":{{\"A\":{},\"B\":{}}},",
                "\"double_reduction\":{{",
                "\"A\":{{\"allowed\":{},\"rate\":{}}},",
                "\"B\":{{\"allowed\":{},\"rate\":{}}}}}}}"
            ),
            markers,
            offspring,
            list(&dosage_a),
            list(&dosage_b),
            gl,
            rec_a,
            rec_b,
            allowed_a,
            rate_a,
            allowed_b,
            rate_b,
        )
    }

    /// The optimum by enumeration over every admissible parental assignment.
    fn enumerated_optimum(t: &Tables) -> f64 {
        let markers = t.markers;
        let counts_a: Vec<usize> = (0..markers).map(|m| t.contrib[0][m].len()).collect();
        let counts_b: Vec<usize> = (0..markers).map(|m| t.contrib[1][m].len()).collect();
        let total: usize =
            counts_a.iter().product::<usize>() * counts_b.iter().product::<usize>();
        assert!(total < 8_000_000, "enumeration too large: {total}");
        let mut xa = vec![0usize; markers];
        let mut xb = vec![0usize; markers];
        let mut best = f64::NEG_INFINITY;
        for n in 0..total {
            let mut rest = n;
            for m in 0..markers {
                xa[m] = rest % counts_a[m];
                rest /= counts_a[m];
            }
            for m in 0..markers {
                xb[m] = rest % counts_b[m];
                rest /= counts_b[m];
            }
            let value = t.evaluate(&xa, &xb);
            if value > best {
                best = value;
            }
        }
        best
    }

    /// Builds a cross at the hard end of the format: every marker carries a
    /// dosage with the most admissible columns for both parents, so the tree is
    /// as wide as the input range allows.
    fn hard_cross(rng: &mut Rng, markers: usize, offspring: usize, dosage: u8) -> String {
        let list = |items: &[u8]| -> String {
            let parts: Vec<String> = items.iter().map(|x| x.to_string()).collect();
            format!("[{}]", parts.join(","))
        };
        let mut gl = String::new();
        for c in 0..offspring {
            if c > 0 {
                gl.push(',');
            }
            gl.push('[');
            for m in 0..markers {
                if m > 0 {
                    gl.push(',');
                }
                let peak = rng.below(5);
                let slope = 0.4 + (rng.below(400) as f64) / 1000.0;
                gl.push('[');
                for d in 0..5 {
                    if d > 0 {
                        gl.push(',');
                    }
                    let distance = (d as i64 - peak as i64).abs() as f64;
                    let jitter = (rng.below(300) as f64) / 1000.0;
                    gl.push_str(&format!("{:.6}", -slope * distance - jitter));
                }
                gl.push(']');
            }
            gl.push(']');
        }
        let rec: String = {
            let parts: Vec<String> = (0..markers - 1)
                .map(|_| format!("{:.6}", 0.02 + 0.3 * (rng.below(1000) as f64) / 1000.0))
                .collect();
            format!("[{}]", parts.join(","))
        };
        let allowed: String = {
            let parts: Vec<String> = (0..markers).map(|_| "true".to_string()).collect();
            format!("[{}]", parts.join(","))
        };
        let rate: String = {
            let parts: Vec<String> = (0..markers)
                .map(|_| format!("{:.6}", 0.02 + 0.2 * (rng.below(1000) as f64) / 1000.0))
                .collect();
            format!("[{}]", parts.join(","))
        };
        format!(
            concat!(
                "{{\"format_version\":1,\"markers\":{},\"offspring\":{},",
                "\"parent_dosage\":{{\"A\":{},\"B\":{}}},",
                "\"child_gl\":[{}],",
                "\"recombination\":{{\"A\":{},\"B\":{}}},",
                "\"double_reduction\":{{",
                "\"A\":{{\"allowed\":{},\"rate\":{}}},",
                "\"B\":{{\"allowed\":{},\"rate\":{}}}}}}}"
            ),
            markers,
            offspring,
            list(&vec![dosage; markers]),
            list(&vec![dosage; markers]),
            gl,
            rec,
            rec,
            allowed,
            rate,
            allowed,
            rate,
        )
    }

    /// Builds a cross the way the fixture looks: a planted pair of parental
    /// haplotypes and a planted path per progeny, with likelihoods peaked at the
    /// dosage those imply. `dosage` sets every marker's dosage, `slope` the
    /// falloff per unit of dosage distance.
    fn planted_cross(
        rng: &mut Rng,
        markers: usize,
        offspring: usize,
        dosage: u8,
        slope: f64,
    ) -> String {
        let mut masks_a = Vec::with_capacity(markers);
        let mut masks_b = Vec::with_capacity(markers);
        for _ in 0..markers {
            masks_a.push(1u8 + rng.below(3) as u8);
            masks_b.push(1u8 + rng.below(3) as u8);
        }
        // Turn a free mask into one with exactly `dosage` bits set.
        let shape = |mask: u8, want: u8| -> u8 {
            let mut out = 0u8;
            let mut held = mask;
            let mut left = want;
            for h in 0..4u8 {
                if left == 0 {
                    break;
                }
                if held & 1 == 1 || left > 3 - h {
                    out |= 1 << h;
                    left -= 1;
                }
                held >>= 1;
            }
            out
        };
        let cols_a: Vec<u8> = masks_a.iter().map(|m| shape(*m, dosage)).collect();
        let cols_b: Vec<u8> = masks_b.iter().map(|m| shape(*m, dosage)).collect();

        let mut gl = String::new();
        for c in 0..offspring {
            if c > 0 {
                gl.push(',');
            }
            gl.push('[');
            let mut a = [0usize, 1];
            let mut b = [0usize, 1];
            for m in 0..markers {
                if m > 0 {
                    gl.push(',');
                }
                // The planted path mostly holds its homologs.
                if m == 0 || rng.below(100) < 25 {
                    a = [rng.below(4), rng.below(4)];
                    b = [rng.below(4), rng.below(4)];
                }
                let peak = usize::from((cols_a[m] >> a[0]) & 1)
                    + usize::from((cols_a[m] >> a[1]) & 1)
                    + usize::from((cols_b[m] >> b[0]) & 1)
                    + usize::from((cols_b[m] >> b[1]) & 1);
                gl.push('[');
                for d in 0..5 {
                    if d > 0 {
                        gl.push(',');
                    }
                    let distance = (d as i64 - peak as i64).abs() as f64;
                    let jitter = (rng.below(50) as f64) / 1000.0;
                    gl.push_str(&format!("{:.6}", -slope * distance - 0.01 * d as f64 - jitter));
                }
                gl.push(']');
            }
            gl.push(']');
        }

        let list = |items: &[u8]| -> String {
            let parts: Vec<String> = items.iter().map(|x| x.to_string()).collect();
            format!("[{}]", parts.join(","))
        };
        let rec: String = {
            let parts: Vec<String> = (0..markers - 1)
                .map(|_| format!("{:.6}", 0.02 + 0.25 * (rng.below(1000) as f64) / 1000.0))
                .collect();
            format!("[{}]", parts.join(","))
        };
        let allowed: String = {
            let parts: Vec<String> = (0..markers).map(|_| "true".to_string()).collect();
            format!("[{}]", parts.join(","))
        };
        let rate: String = {
            let parts: Vec<String> = (0..markers)
                .map(|_| format!("{:.6}", 0.02 + 0.15 * (rng.below(1000) as f64) / 1000.0))
                .collect();
            format!("[{}]", parts.join(","))
        };
        format!(
            concat!(
                "{{\"format_version\":1,\"markers\":{},\"offspring\":{},",
                "\"parent_dosage\":{{\"A\":{},\"B\":{}}},",
                "\"child_gl\":[{}],",
                "\"recombination\":{{\"A\":{},\"B\":{}}},",
                "\"double_reduction\":{{",
                "\"A\":{{\"allowed\":{},\"rate\":{}}},",
                "\"B\":{{\"allowed\":{},\"rate\":{}}}}}}}"
            ),
            markers,
            offspring,
            list(&vec![dosage; markers]),
            list(&vec![dosage; markers]),
            gl,
            rec,
            rec,
            allowed,
            rate,
            allowed,
            rate,
        )
    }

    /// End to end on crosses at the top of the input range. Ignored: it takes
    /// minutes, and it is how the effort budget was chosen.
    #[test]
    #[ignore]
    fn solve_on_planted_crosses() {
        let mut rng = Rng::new(0xC0FFEE);
        for (markers, offspring, dosage, slope) in
            [(28usize, 32usize, 2u8, 2.0f64), (28, 16, 2, 2.0), (20, 32, 1, 2.0)]
        {
            let text = planted_cross(&mut rng, markers, offspring, dosage, slope);
            let cross = parse_cross(&text).expect("generated cross must be valid");
            let start = std::time::Instant::now();
            let answer = solve(&cross);
            let elapsed = start.elapsed();
            let total = score(&cross, &answer).expect("the scorer must accept the answer");
            let mut again = answer.clone();
            canonicalize(&mut again);
            assert_eq!(again, answer, "M={markers} N={offspring}: not canonical");
            let tables = Tables::build(&cross);
            let mut before = f64::NEG_INFINITY;
            for seed in 0..6u64 {
                let (value, _, _) = search::search(&tables, 24, 24, 0x9e3779b97f4a7c15 + seed * 7919);
                if value > before {
                    before = value;
                }
            }
            assert!(
                total >= before - 1e-9,
                "M={markers} N={offspring}: the pipeline scores {total:.6}, below the {before:.6} the local search reaches on its own"
            );
            println!(
                "M={markers} N={offspring} d={dosage} s={slope}: local={before:.6} total={total:.6} elapsed={elapsed:?}"
            );
        }
    }

    /// The branch and bound on planted crosses at the top of the input range.
    /// Ignored: it takes minutes, it is how the node budget was chosen.
    #[test]
    #[ignore]
    fn planted_scaling() {
        let mut rng = Rng::new(0xC0FFEE);
        let budget: u64 = std::env::var("SCALE_BUDGET")
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or(5_000_000);
        for (markers, offspring, dosage, slope) in [
            (28usize, 32usize, 2u8, 2.0f64),
            (28, 32, 2, 1.0),
            (28, 16, 2, 2.0),
            (20, 32, 1, 2.0),
        ] {
            let text = planted_cross(&mut rng, markers, offspring, dosage, slope);
            let cross = parse_cross(&text).expect("generated cross must be valid");
            let t = Tables::build(&cross);
            let (seeded, xa, xb) = search::search(&t, 24, 24, 0x9e3779b97f4a7c15);
            let mut branch = Branch::new(&t, seeded - 1e-8, &xa, &xb);
            branch.slack = 1e-8;
            let start = std::time::Instant::now();
            let finished = branch.run(budget);
            println!(
                "M={markers} N={offspring} d={dosage} s={slope}: finished={finished} nodes={} pruned={} best={:.4} gap={:.3e} ties={} elapsed={:?}",
                branch.nodes,
                branch.pruned,
                branch.best_value,
                branch.best_value - seeded,
                branch.ties.len(),
                start.elapsed()
            );
        }
    }

    /// How the branch and bound behaves at the top of the input range. Ignored:
    /// it takes minutes, it is how the node budget was chosen.
    #[test]
    #[ignore]
    fn scaling_at_the_top_of_the_range() {
        let mut rng = Rng::new(0x5ca1ab1e);
        let cases: Vec<(usize, usize, u8)> = match std::env::var("SCALE_CASES") {
            Ok(text) => text
                .split(';')
                .map(|part| {
                    let bits: Vec<usize> = part.split(',').map(|x| x.parse().unwrap()).collect();
                    (bits[0], bits[1], bits[2] as u8)
                })
                .collect(),
            Err(_) => vec![(28, 32, 2), (28, 32, 1), (28, 8, 2), (20, 32, 2)],
        };
        let budget: u64 = std::env::var("SCALE_BUDGET")
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or(40_000_000);
        for (markers, offspring, dosage) in cases {
            let text = hard_cross(&mut rng, markers, offspring, dosage);
            let cross = parse_cross(&text).expect("generated cross must be valid");
            let t = Tables::build(&cross);
            let (seeded, xa, xb) = search::search(&t, 24, 24, 0x9e3779b97f4a7c15);
            let mut branch = Branch::new(&t, seeded - 1e-8, &xa, &xb);
            branch.slack = 1e-8;
            let start = std::time::Instant::now();
            let finished = branch.run(budget);
            println!(
                "M={markers} N={offspring} d={dosage}: finished={finished} nodes={} pruned={} best={:.6} gap={:.2e} ties={} elapsed={:?}",
                branch.nodes,
                branch.pruned,
                branch.best_value,
                branch.best_value - seeded,
                branch.ties.len(),
                start.elapsed()
            );
        }
    }

    #[test]
    fn branch_and_bound_matches_enumeration() {
        let mut rng = Rng::new(0xa11ce);
        for case in 0..12 {
            let markers = 12;
            let offspring = 8;
            let free = 2 + case % 2;
            let text = random_cross(&mut rng, markers, offspring, free);
            let cross = parse_cross(&text).expect("generated cross must be valid");
            let t = Tables::build(&cross);

            let (seeded, xa, xb) = search::search(&t, 8, 8, 0x1234 + case as u64);
            let mut branch = Branch::new(&t, seeded - 1e-9, &xa, &xb);
            let finished = branch.run(50_000_000);
            assert!(finished, "case {case}: branch and bound ran out of budget");

            let truth = enumerated_optimum(&t);
            assert!(
                scores_equal(branch.best_value, truth),
                "case {case}: branch and bound reports {:.9} where enumeration finds {:.9}",
                branch.best_value,
                truth
            );
            assert!(
                branch.best_value >= truth - 1e-9,
                "case {case}: branch and bound is below the optimum"
            );
        }
    }

    #[test]
    fn the_pipeline_reports_a_valid_scoring_canonical_answer() {
        let mut rng = Rng::new(0xb0b);
        for case in 0..8 {
            let markers = 12;
            let offspring = 8;
            let text = random_cross(&mut rng, markers, offspring, 2 + case % 2);
            let cross = parse_cross(&text).expect("generated cross must be valid");
            let t = Tables::build(&cross);
            let truth = enumerated_optimum(&t);

            let answer = solve(&cross);
            let total = score(&cross, &answer).expect("the scorer must accept the answer");
            assert!(
                scores_equal(total, truth),
                "case {case}: pipeline scores {total:.9}, enumeration finds {truth:.9}"
            );

            let mut again = answer.clone();
            canonicalize(&mut again);
            assert_eq!(again, answer, "case {case}: the answer is not canonical");
            let key = distinct_key(&answer);
            let flattened: Vec<u8> = answer
                .haplotypes(Parent::A)
                .rows()
                .iter()
                .chain(answer.haplotypes(Parent::B).rows().iter())
                .flat_map(|row| row.iter().copied())
                .collect();
            assert_eq!(&key[..flattened.len()], &flattened[..]);

            let text = encode(&answer, total);
            let decoded = decode(&text, markers, offspring).expect("document must decode");
            assert_eq!(decoded.answer, answer, "case {case}: document round trip");
        }
    }
}
