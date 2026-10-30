//! Verifier-owned judge. Sole grading authority.
//!
//! The judge never links candidate code. It owns every expected value, reads
//! each probe record, applies the literal per-case predicate that V1 expressed
//! as `assert_eq!`, and writes a verdict file. Nothing the probe printed, and no
//! probe exit status, reaches this binary.

mod reference;

use bt_model::{cases, record, RawCalibration, RawError, Value};
use std::collections::BTreeMap;
use std::path::Path;

fn int(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        _ => None,
    }
}

fn cal(v: &Value) -> Option<&RawCalibration> {
    match v {
        Value::Cal(c) => Some(c),
        _ => None,
    }
}

fn biases_eq(c: &RawCalibration, want: &[(&str, i64)]) -> bool {
    c.phase_biases_md.len() == want.len()
        && c.phase_biases_md
            .iter()
            .zip(want)
            .all(|((n, v), (wn, wv))| n == wn && v == wv)
}

fn ints_eq(vals: &[Value], want: &[i64]) -> bool {
    vals.len() == want.len() && vals.iter().zip(want).all(|(v, w)| int(v) == Some(*w))
}

/// The V1 assertions for each case, expressed once and applied both to the
/// reference values (model-drift guard) and to the recorded values (grading).
///
/// This function and its comments are the sole home of the expected results and
/// of the V1 `Public requirement` / `Semantic behavior` provenance. None of it
/// appears in the `bt_model` crate, which is compiled into the probe the graded
/// account executes.
fn literal_ok(id: &str, vals: &[Value]) -> bool {
    match id {
        // Public requirement: distance, clock, and antenna biases are recovered jointly.
        // Semantic behavior: the global candidate is selected from both evidence streams.
        // Rejects: a PBR-only or RTT-only fit, and per-observation fitting that
        // never reconciles the two streams against one parameter set.
        "joint_global_recovery" => cal(&vals[0]).is_some_and(|c| {
            (c.distance_mm, c.clock_bias_ps) == (1_350, -120)
                && biases_eq(c, &[("ant-a", 179_500), ("ant-b", -179_200)])
        }),
        // Public requirement: PBR cannot be discarded when RTT has a distance/clock ambiguity.
        // Semantic behavior: two exact RTT fits are separated only by frequency phase.
        // Rejects: an RTT-only fit, which scores both distances identically here
        // and returns whichever the enumeration reached first.
        "pbr_resolves_rtt_clock_ambiguity" => {
            cal(&vals[0]).is_some_and(|c| c.distance_mm == 200)
                && cal(&vals[1]).is_some_and(|c| c.clock_bias_ps == 0)
        }
        // Public requirement: RTT cannot be discarded when PBR is locally ambiguous.
        // Semantic behavior: a zero-slope calibration channel leaves distance to RTT.
        // Rejects: a PBR-only fit, which has no distance evidence at all on a
        // zero-slope channel and so cannot choose.
        "rtt_resolves_pbr_ambiguity" => cal(&vals[0]).is_some_and(|c| c.distance_mm == 200),
        // Public requirement: candidate endpoints are inclusive.
        // Semantic behavior: the winning value may be the final item of every supplied grid.
        // Rejects: exclusive or half-open grid iteration, which never evaluates the
        // final sorted entry and therefore cannot reach the answer.
        "bounded_grid_endpoints_included" => cal(&vals[0]).is_some_and(|c| {
            (c.distance_mm, c.clock_bias_ps) == (777, 45)
                && c.phase_biases_md.len() > 1
                && c.phase_biases_md[1].1 == 176_000
        }),
        // Public requirement: observation storage order is non-semantic.
        // Semantic behavior: shuffling inputs gives byte-identical canonical output.
        // Rejects: input-order dependence -- first-match grid scanning, or
        // predictions emitted in observation arrival order.
        "channel_input_order_is_ignored" => {
            cal(&vals[0]).is_some() && cal(&vals[1]).is_some() && vals[0] == vals[1]
        }
        // Public requirement: canonical tie rules decide globally equal scores.
        // Semantic behavior: distance, clock, then complete path vector are lexicographic.
        // Rejects: first-minimum tie handling, which returns whichever combination
        // the loop reached first, and paths emitted in supplied order.
        "deterministic_empty_evidence_tie" => cal(&vals[0]).is_some_and(|c| {
            (c.distance_mm, c.clock_bias_ps) == (3, -2) && biases_eq(c, &[("a", -9), ("z", -8)])
        }),
        // Public requirement: PBR error is circular at the modulo boundary.
        // Semantic behavior: values on opposite signed representatives are one unit apart.
        // Rejects: a linear absolute-difference residual, positive-only modulo, and
        // the wrong -180000/180000 boundary ownership.
        "circular_error_drives_selection" => cal(&vals[0])
            .is_some_and(|c| !c.phase_biases_md.is_empty() && c.phase_biases_md[0].1 == 179_999),
        // Public requirement: returned predictions are model-derived and canonical.
        // Semantic behavior: every hidden-like channel value is recomputed for the winner.
        // Rejects: echoing the measurements back as predictions, and
        // truncating instead of rounding, whose drift shows at these
        // magnitudes. Floating point is forbidden by the contract rather than
        // caught here: every intermediate stays exactly representable.
        "predictions_are_load_bearing" => cal(&vals[0]).is_some_and(|c| {
            // Derived from the candidate's OWN returned biases, at the literal
            // distance 1111, exactly as V1 derived it.
            let Some(cases::Call::Recover(p)) = cases::plan(id).and_then(|v| v.into_iter().next())
            else {
                return false;
            };
            let mut obs = p.pbr.clone();
            obs.sort();
            let map: BTreeMap<_, _> = c.phase_biases_md.iter().cloned().collect();
            let mut expected = Vec::new();
            for o in &obs {
                match map.get(&o.path_id) {
                    Some(b) => expected.push(bt_model::phase(o.frequency_khz, 1_111, *b)),
                    None => return false,
                }
            }
            c.pbr_predictions_md == expected
                && c.pbr_predictions_md.iter().any(|v| v.abs() > 100_000)
        }),
        // Public requirement: path identity is part of PBR calibration.
        // Semantic behavior: swapping two opposing path biases cannot preserve the score.
        // Rejects: one global antenna bias shared across paths, which averages an
        // equal-and-opposite pair away.
        "tied_antenna_paths_remain_distinct" => cal(&vals[0])
            .is_some_and(|c| biases_eq(c, &[("ant-a", 70_000), ("ant-b", -70_000)])),
        // Public requirement: the compound case combines all exactness rules.
        // Semantic behavior: wrap, shuffled channels, RTT ambiguity, paths and full tie act together.
        // Rejects: any solver right on one rule but not their conjunction --
        // positive-only modulo, a shared bias, an RTT-only fit, or order-dependent
        // scanning each land on a different calibration.
        "compound_wrap_rtt_paths_and_order" => cal(&vals[0]).is_some_and(|c| {
            (c.distance_mm, c.clock_bias_ps) == (2_499, 123)
                && biases_eq(c, &[("ant-a", 179_999), ("ant-b", -179_999)])
                && c.rtt_predictions_ps.len() == 2
        }),
        // Public requirement: wrapping uses the exact signed half-open interval.
        // Semantic behavior: both representatives of the modulo boundary canonicalize.
        // Rejects: positive-only modulo, and the off-by-one ownership that maps the
        // boundary onto the excluded representative.
        "signed_half_open_wrap_boundaries" => {
            ints_eq(vals, &[0, 179_999, -180_000, 179_999, -180_000])
        }
        // Public requirement: RTT uses the supplied integer propagation model.
        // Semantic behavior: bias and path delay are added after rational rounding.
        // Rejects: applying the model terms in a different order, and
        // truncating instead of rounding, both of which move the result at
        // these inputs. Floating point is forbidden by the contract rather than
        // caught here.
        "exact_rtt_model_order" => ints_eq(vals, &[5, 6_671]),
        // Public requirement: phase propagation uses exact integer rational
        // arithmetic throughout the documented i64 input domain.
        // Semantic behavior: this large distance rounds to 120690 millidegrees
        // after wrapping; f64 loses one unit before the wrap.
        // Rejects: converting the i128 propagation numerator to floating point.
        "large_integer_phase_rounding_is_exact" => ints_eq(vals, &[120_690]),
        // Public requirement: distance is physically bounded to nonnegative candidates.
        // Semantic behavior: negative values fail explicitly rather than wrapping.
        // Rejects: unchecked arithmetic that propagates the negative distance into a
        // phase or timing figure instead of refusing it.
        "negative_distance_rejected" => {
            vals[0] == Value::Err(RawError::NegativeDistance)
                && vals[1] == Value::Err(RawError::NegativeDistance)
        }
        // Public requirement: all input path identities need supplied bias grids.
        // Semantic behavior: a missing named path is not silently assigned zero bias.
        // Rejects: accepting the missing path by defaulting its bias to zero, which
        // invents a calibration for a path that was never characterised.
        "missing_path_rejected" => vals[0] == Value::Err(RawError::MissingPath("x".to_string())),
        // Public requirement: every named antenna path carries a nonempty bias grid.
        // Semantic behavior: a grid that exists but offers no candidate value
        // characterises nothing, so the problem is refused rather than answered
        // from the remaining evidence.
        // Rejects: reading an empty grid as a zero bias, and dropping the path so
        // the calibration comes from whatever evidence is left.
        "empty_bias_grid_rejected" => vals[0] == Value::Err(RawError::EmptyGrid),
        // Public requirement: the winner is recovered jointly from both evidence
        // streams, with both maxima and the full path bias vector load-bearing.
        // Semantic behavior: at the joint optimum the first channel prediction sits
        // exactly on the signed boundary, the other maxima disambiguate the clock,
        // and the tied path bias pair is chosen canonically.
        // Rejects: comparing only the residual sums; taking the first minimum
        // reached; assuming the winning maxima are the smallest available;
        // truncating instead of rounding; and mis-owning the modulo boundary.
        "compound_joint_maxima_and_boundary_tie" => cal(&vals[0]).is_some_and(|c| {
            (c.distance_mm, c.clock_bias_ps) == (1_200, 61)
                && biases_eq(c, &[("ant-a", 97_451), ("ant-b", 100)])
                && c.pbr_predictions_md == vec![-180_000, 151_817, -162_071, 44_796]
                && c.rtt_predictions_ps == vec![8_050, 8_100]
        }),
        _ => false,
    }
}

/// Reference values for one case, from the verifier's own model only.
fn reference_values(id: &str) -> Vec<Value> {
    cases::plan(id)
        .expect("verifier-owned case")
        .iter()
        .map(reference::run)
        .collect()
}

/// Read one probe record. Refuses symlinks, non-files and oversized files.
fn read_record(dir: &Path, id: &str) -> Result<Vec<Value>, &'static str> {
    let path = dir.join(format!("{id}.rec"));
    let meta = std::fs::symlink_metadata(&path).map_err(|_| "record missing")?;
    if meta.file_type().is_symlink() {
        return Err("record is a symlink");
    }
    if !meta.is_file() {
        return Err("record is not a regular file");
    }
    if meta.len() > record::MAX_RECORD_BYTES {
        return Err("record too large");
    }
    let arity = cases::arity(id).ok_or("unknown case")?;
    let text = std::fs::read_to_string(&path).map_err(|_| "record unreadable")?;
    record::decode(&text, arity).ok_or("record malformed")
}

fn grade_one(dir: &Path, id: &str) -> Result<(), &'static str> {
    if !literal_ok(id, &reference_values(id)) {
        return Err("model-drift: expectation no longer follows from the frozen model");
    }
    let vals = read_record(dir, id)?;
    if literal_ok(id, &vals) {
        Ok(())
    } else {
        Err("values")
    }
}

fn grade_all(dir: &Path, out: &mut String) -> usize {
    let mut bad = 0;
    for id in cases::CASES {
        match grade_one(dir, id) {
            Ok(()) => out.push_str(&format!("PASS {id}\n")),
            Err(reason) => {
                bad += 1;
                out.push_str(&format!("FAIL {id} {reason}\n"));
            }
        }
    }
    out.push_str(&format!("TOTAL {} FAILED {}\n", cases::CASES.len(), bad));
    bad
}

/// Negative control. Stages the full correct record set from the reference,
/// corrupts exactly one functional value, grades, and requires exactly that one
/// failure. No candidate code participates.
fn selftest(dir: &Path) -> i32 {
    const TARGET: &str = "compound_joint_maxima_and_boundary_tie";
    if std::fs::create_dir_all(dir).is_err() {
        return 2;
    }
    for id in cases::CASES {
        let mut vals = reference_values(id);
        if *id == TARGET {
            // A distance no candidate grid contains and no legal answer can be.
            if let Value::Cal(c) = &mut vals[0] {
                c.distance_mm = i64::MAX;
            } else {
                return 2;
            }
        }
        if std::fs::write(dir.join(format!("{id}.rec")), record::encode(&vals)).is_err() {
            return 2;
        }
    }
    let mut out = String::new();
    let bad = grade_all(dir, &mut out);
    print!("{out}");
    let saw_target = out.contains(&format!("FAIL {TARGET} "));
    if bad == 1 && saw_target {
        println!("SELFTEST ok: injected defect observed");
        0
    } else {
        println!("SELFTEST failed: expected exactly one failure at {TARGET}");
        1
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match (a.len(), a.get(1).map(String::as_str)) {
        (3, Some("--selftest")) => std::process::exit(selftest(Path::new(&a[2]))),
        (4, Some("--grade")) => {
            let mut out = String::new();
            let bad = grade_all(Path::new(&a[2]), &mut out);
            print!("{out}");
            if std::fs::write(&a[3], &out).is_err() {
                std::process::exit(2)
            }
            std::process::exit(if bad == 0 { 0 } else { 1 })
        }
        _ => std::process::exit(2),
    }
}
