//! Verifier-owned reference implementation of the channel model.
//!
//! This is the judge's own independent implementation. It exists for two
//! purposes and no others:
//!
//! 1. a model-drift guard — the judge re-derives every case from this reference
//!    and refuses to grade at all if its literal expectations no longer follow
//!    from the model, and
//! 2. staging correct records for `--selftest`.
//!
//! No candidate code participates in either. This file is never linked into the
//! probe and is unreadable by the graded account.

use bt_model::{
    cases::Call, phase, round_ratio, wrap, Problem, RawCalibration, RawError, Value, LIGHT_SPEED,
};
use std::collections::BTreeMap;

fn wrap_phase_md(value: i128) -> Result<i64, RawError> {
    Ok(wrap(value))
}

fn predict_phase_md(f: i64, d: i64, bias: i64) -> Result<i64, RawError> {
    if d < 0 {
        return Err(RawError::NegativeDistance);
    }
    Ok(phase(f, d, bias))
}

fn predict_rtt_ps(d: i64, clock: i64, path_delay: i64) -> Result<i64, RawError> {
    if d < 0 {
        return Err(RawError::NegativeDistance);
    }
    let n = 2_000_000_000_i128 * d as i128;
    i64::try_from(round_ratio(n, LIGHT_SPEED) + clock as i128 + path_delay as i128)
        .map_err(|_| RawError::ArithmeticOverflow)
}

fn circular_error(a: i64, b: i64) -> i128 {
    (wrap(a as i128 - b as i128) as i128).abs()
}

type Scores = (i128, i128, i128, i128, Vec<i64>, Vec<i64>);

fn evaluate(
    problem: &Problem,
    distance: i64,
    clock: i64,
    biases: &[(String, i64)],
) -> Result<Scores, RawError> {
    let map: BTreeMap<_, _> = biases.iter().cloned().collect();
    let mut pbr = problem.pbr.clone();
    pbr.sort();
    let mut rtt = problem.rtt.clone();
    rtt.sort();
    let (mut ps, mut rs, mut pm, mut rm) = (0_i128, 0_i128, 0_i128, 0_i128);
    let mut pp = Vec::with_capacity(pbr.len());
    for obs in pbr {
        let bias = *map
            .get(&obs.path_id)
            .ok_or_else(|| RawError::MissingPath(obs.path_id.clone()))?;
        let predicted = predict_phase_md(obs.frequency_khz, distance, bias)?;
        let error = circular_error(predicted, obs.measured_phase_md);
        ps += error;
        pm = pm.max(error);
        pp.push(predicted);
    }
    let mut rp = Vec::with_capacity(rtt.len());
    for obs in rtt {
        let predicted = predict_rtt_ps(distance, clock, obs.path_delay_ps)?;
        let error = (predicted as i128 - obs.measured_rtt_ps as i128).abs();
        rs += error;
        rm = rm.max(error);
        rp.push(predicted);
    }
    Ok((ps, rs, pm, rm, pp, rp))
}

type Key = (i128, i128, i128, i128, i64, i64, Vec<(String, i64)>);
type Best = Option<(Key, Vec<i64>, Vec<i64>)>;

fn visit_biases(
    problem: &Problem,
    paths: &[(String, Vec<i64>)],
    at: usize,
    chosen: &mut Vec<(String, i64)>,
    distance: i64,
    clock: i64,
    best: &mut Best,
) -> Result<(), RawError> {
    if at != paths.len() {
        for &bias in &paths[at].1 {
            chosen.push((paths[at].0.clone(), bias));
            visit_biases(problem, paths, at + 1, chosen, distance, clock, best)?;
            chosen.pop();
        }
        return Ok(());
    }
    let (ps, rs, pm, rm, pp, rp) = evaluate(problem, distance, clock, chosen)?;
    let key = (ps, rs, pm, rm, distance, clock, chosen.clone());
    if best.as_ref().map_or(true, |(old, _, _)| key < *old) {
        *best = Some((key, pp, rp));
    }
    Ok(())
}

fn recover(problem: &Problem) -> Result<RawCalibration, RawError> {
    if problem.distance_candidates_mm.is_empty() || problem.clock_bias_candidates_ps.is_empty() {
        return Err(RawError::EmptyGrid);
    }
    let mut distances = problem.distance_candidates_mm.clone();
    distances.sort();
    distances.dedup();
    let mut clocks = problem.clock_bias_candidates_ps.clone();
    clocks.sort();
    clocks.dedup();
    if distances.iter().any(|d| *d < 0) {
        return Err(RawError::NegativeDistance);
    }
    let mut paths = Vec::new();
    for (name, grid) in &problem.phase_bias_candidates_md {
        if grid.is_empty() {
            return Err(RawError::EmptyGrid);
        }
        let mut values = grid.clone();
        values.sort();
        values.dedup();
        paths.push((name.clone(), values));
    }
    for obs in &problem.pbr {
        if !problem.phase_bias_candidates_md.contains_key(&obs.path_id) {
            return Err(RawError::MissingPath(obs.path_id.clone()));
        }
    }
    let mut best = None;
    for distance in distances {
        for &clock in &clocks {
            visit_biases(problem, &paths, 0, &mut Vec::new(), distance, clock, &mut best)?;
        }
    }
    let (key, pbr_predictions_md, rtt_predictions_ps) = best.ok_or(RawError::EmptyGrid)?;
    Ok(RawCalibration {
        distance_mm: key.4,
        clock_bias_ps: key.5,
        phase_biases_md: key.6,
        pbr_predictions_md,
        rtt_predictions_ps,
    })
}

fn int_or_err(r: Result<i64, RawError>) -> Value {
    match r {
        Ok(v) => Value::Int(v),
        Err(e) => Value::Err(e),
    }
}

/// Reference values for one call.
pub fn run(call: &Call) -> Value {
    match call {
        Call::Recover(p) => match recover(p) {
            Ok(c) => Value::Cal(c),
            Err(e) => Value::Err(e),
        },
        Call::Wrap(v) => int_or_err(wrap_phase_md(*v)),
        Call::PredictPhase(f, d, b) => int_or_err(predict_phase_md(*f, *d, *b)),
        Call::PredictRtt(d, c, p) => int_or_err(predict_rtt_ps(*d, *c, *p)),
    }
}
