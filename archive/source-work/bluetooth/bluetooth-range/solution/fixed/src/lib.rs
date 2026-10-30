
use std::collections::BTreeMap;

pub const LIGHT_SPEED: i128 = 299_792_458;
pub const FULL_PHASE_MD: i64 = 360_000;
pub const HALF_PHASE_MD: i64 = 180_000;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PbrObservation { pub frequency_khz: i64, pub path_id: String, pub measured_phase_md: i64 }
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RttObservation { pub path_id: String, pub path_delay_ps: i64, pub measured_rtt_ps: i64 }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub distance_candidates_mm: Vec<i64>, pub clock_bias_candidates_ps: Vec<i64>,
    pub phase_bias_candidates_md: BTreeMap<String, Vec<i64>>, pub pbr: Vec<PbrObservation>, pub rtt: Vec<RttObservation>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Calibration {
    pub distance_mm: i64, pub clock_bias_ps: i64, pub phase_biases_md: Vec<(String, i64)>,
    pub pbr_predictions_md: Vec<i64>, pub rtt_predictions_ps: Vec<i64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error { EmptyGrid, MissingPath(String), NegativeDistance, ArithmeticOverflow }

pub fn wrap_phase_md(value: i128) -> Result<i64, Error> {
    let wrapped = value.rem_euclid(FULL_PHASE_MD as i128);
    let signed = if wrapped >= HALF_PHASE_MD as i128 { wrapped - FULL_PHASE_MD as i128 } else { wrapped };
    i64::try_from(signed).map_err(|_| Error::ArithmeticOverflow)
}
fn round_ratio(n: i128, d: i128) -> Result<i128, Error> {
    n.checked_add(d / 2).map(|v| v / d).ok_or(Error::ArithmeticOverflow)
}
pub fn predict_phase_md(f: i64, d: i64, bias: i64) -> Result<i64, Error> {
    if d < 0 { return Err(Error::NegativeDistance); }
    let n = 720_000_i128.checked_mul(f as i128).and_then(|v| v.checked_mul(d as i128)).ok_or(Error::ArithmeticOverflow)?;
    wrap_phase_md(round_ratio(n, LIGHT_SPEED)?.checked_add(bias as i128).ok_or(Error::ArithmeticOverflow)?)
}
pub fn predict_rtt_ps(d: i64, clock: i64, path_delay: i64) -> Result<i64, Error> {
    if d < 0 { return Err(Error::NegativeDistance); }
    let n = 2_000_000_000_i128.checked_mul(d as i128).ok_or(Error::ArithmeticOverflow)?;
    i64::try_from(round_ratio(n, LIGHT_SPEED)? + clock as i128 + path_delay as i128).map_err(|_| Error::ArithmeticOverflow)
}
fn circular_error(a: i64, b: i64) -> Result<i128, Error> { Ok((wrap_phase_md(a as i128 - b as i128)? as i128).abs()) }

fn evaluate(problem: &Problem, distance: i64, clock: i64, biases: &[(String, i64)]) -> Result<(i128, i128, i128, i128, Vec<i64>, Vec<i64>), Error> {
    let map: BTreeMap<_, _> = biases.iter().cloned().collect();
    let mut pbr = problem.pbr.clone(); pbr.sort();
    let mut rtt = problem.rtt.clone(); rtt.sort();
    let (mut ps, mut rs, mut pm, mut rm) = (0_i128, 0_i128, 0_i128, 0_i128);
    let mut pp = Vec::with_capacity(pbr.len());
    for obs in pbr {
        let bias = *map.get(&obs.path_id).ok_or_else(|| Error::MissingPath(obs.path_id.clone()))?;
        let predicted = predict_phase_md(obs.frequency_khz, distance, bias)?;
        let error = circular_error(predicted, obs.measured_phase_md)?;
        ps += error; pm = pm.max(error); pp.push(predicted);
    }
    let mut rp = Vec::with_capacity(rtt.len());
    for obs in rtt {
        let predicted = predict_rtt_ps(distance, clock, obs.path_delay_ps)?;
        let error = (predicted as i128 - obs.measured_rtt_ps as i128).abs();
        rs += error; rm = rm.max(error); rp.push(predicted);
    }
    Ok((ps, rs, pm, rm, pp, rp))
}

fn visit_biases(problem: &Problem, paths: &[(String, Vec<i64>)], at: usize, chosen: &mut Vec<(String, i64)>, distance: i64, clock: i64, best: &mut Option<((i128,i128,i128,i128,i64,i64,Vec<(String,i64)>),Vec<i64>,Vec<i64>)>) -> Result<(), Error> {
    if at != paths.len() {
        for &bias in &paths[at].1 { chosen.push((paths[at].0.clone(), bias)); visit_biases(problem, paths, at + 1, chosen, distance, clock, best)?; chosen.pop(); }
        return Ok(());
    }
    let (ps,rs,pm,rm,pp,rp) = evaluate(problem,distance,clock,chosen)?;
    let key = (ps,rs,pm,rm,distance,clock,chosen.clone());
    if best.as_ref().map_or(true, |(old,_,_)| key < *old) { *best = Some((key,pp,rp)); }
    Ok(())
}

pub fn recover(problem: &Problem) -> Result<Calibration, Error> {
    if problem.distance_candidates_mm.is_empty() || problem.clock_bias_candidates_ps.is_empty() { return Err(Error::EmptyGrid); }
    let mut distances = problem.distance_candidates_mm.clone(); distances.sort(); distances.dedup();
    let mut clocks = problem.clock_bias_candidates_ps.clone(); clocks.sort(); clocks.dedup();
    if distances.iter().any(|d| *d < 0) { return Err(Error::NegativeDistance); }
    let mut paths = Vec::new();
    for (name, grid) in &problem.phase_bias_candidates_md {
        if grid.is_empty() { return Err(Error::EmptyGrid); }
        let mut values = grid.clone(); values.sort(); values.dedup(); paths.push((name.clone(), values));
    }
    for obs in &problem.pbr { if !problem.phase_bias_candidates_md.contains_key(&obs.path_id) { return Err(Error::MissingPath(obs.path_id.clone())); } }
    let mut best = None;
    for distance in distances { for &clock in &clocks { visit_biases(problem,&paths,0,&mut Vec::new(),distance,clock,&mut best)?; } }
    let (key,pbr_predictions_md,rtt_predictions_ps) = best.ok_or(Error::EmptyGrid)?;
    Ok(Calibration { distance_mm:key.4, clock_bias_ps:key.5, phase_biases_md:key.6, pbr_predictions_md, rtt_predictions_ps })
}
