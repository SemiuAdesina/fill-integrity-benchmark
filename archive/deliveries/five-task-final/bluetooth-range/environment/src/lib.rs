
use std::collections::BTreeMap;

pub const LIGHT_SPEED: i128 = 299_792_458;
pub const FULL_PHASE_MD: i64 = 360_000;
pub const HALF_PHASE_MD: i64 = 180_000;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PbrObservation {
    pub frequency_khz: i64,
    pub path_id: String,
    pub measured_phase_md: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RttObservation {
    pub path_id: String,
    pub path_delay_ps: i64,
    pub measured_rtt_ps: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub distance_candidates_mm: Vec<i64>,
    pub clock_bias_candidates_ps: Vec<i64>,
    pub phase_bias_candidates_md: BTreeMap<String, Vec<i64>>,
    pub pbr: Vec<PbrObservation>,
    pub rtt: Vec<RttObservation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Calibration {
    pub distance_mm: i64,
    pub clock_bias_ps: i64,
    pub phase_biases_md: Vec<(String, i64)>,
    pub pbr_predictions_md: Vec<i64>,
    pub rtt_predictions_ps: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    EmptyGrid,
    MissingPath(String),
    NegativeDistance,
    ArithmeticOverflow,
}

pub fn wrap_phase_md(value: i128) -> Result<i64, Error> {
    let wrapped = value.rem_euclid(FULL_PHASE_MD as i128);
    i64::try_from(wrapped).map_err(|_| Error::ArithmeticOverflow)
}

fn round_ratio(numerator: i128, denominator: i128) -> Result<i128, Error> {
    numerator
        .checked_add(denominator / 2)
        .map(|n| n / denominator)
        .ok_or(Error::ArithmeticOverflow)
}

pub fn predict_phase_md(frequency_khz: i64, distance_mm: i64, bias_md: i64) -> Result<i64, Error> {
    if distance_mm < 0 { return Err(Error::NegativeDistance); }
    let n = 720_000_i128
        .checked_mul(frequency_khz as i128).and_then(|v| v.checked_mul(distance_mm as i128))
        .ok_or(Error::ArithmeticOverflow)?;
    wrap_phase_md(round_ratio(n, LIGHT_SPEED)?.checked_add(bias_md as i128).ok_or(Error::ArithmeticOverflow)?)
}

pub fn predict_rtt_ps(distance_mm: i64, clock_bias_ps: i64, path_delay_ps: i64) -> Result<i64, Error> {
    if distance_mm < 0 { return Err(Error::NegativeDistance); }
    let n = 2_000_000_000_i128.checked_mul(distance_mm as i128).ok_or(Error::ArithmeticOverflow)?;
    let value = round_ratio(n, LIGHT_SPEED)? + clock_bias_ps as i128 + path_delay_ps as i128;
    i64::try_from(value).map_err(|_| Error::ArithmeticOverflow)
}

pub fn recover(problem: &Problem) -> Result<Calibration, Error> {
    // Deliberately incomplete baseline: use RTT only and take the first phase
    // candidate for each path. The public fixtures expose why this is invalid.
    if problem.distance_candidates_mm.is_empty() || problem.clock_bias_candidates_ps.is_empty() {
        return Err(Error::EmptyGrid);
    }
    let mut best: Option<(i128, i64, i64)> = None;
    for &distance in &problem.distance_candidates_mm {
        if distance < 0 { return Err(Error::NegativeDistance); }
        for &clock in &problem.clock_bias_candidates_ps {
            let mut residual = 0_i128;
            for obs in &problem.rtt {
                residual += (predict_rtt_ps(distance, clock, obs.path_delay_ps)? as i128 - obs.measured_rtt_ps as i128).abs();
            }
            let key = (residual, distance, clock);
            if best.as_ref().map_or(true, |old| key < *old) { best = Some(key); }
        }
    }
    let (_, distance_mm, clock_bias_ps) = best.ok_or(Error::EmptyGrid)?;
    let mut phase_biases_md = Vec::new();
    for (path, grid) in &problem.phase_bias_candidates_md {
        let bias = *grid.first().ok_or(Error::EmptyGrid)?;
        phase_biases_md.push((path.clone(), bias));
    }
    let bias_map: BTreeMap<_, _> = phase_biases_md.iter().cloned().collect();
    let mut pbr = problem.pbr.clone(); pbr.sort();
    let mut rtt = problem.rtt.clone(); rtt.sort();
    let mut pbr_predictions_md = Vec::new();
    for obs in pbr { pbr_predictions_md.push(predict_phase_md(obs.frequency_khz, distance_mm, *bias_map.get(&obs.path_id).ok_or_else(|| Error::MissingPath(obs.path_id.clone()))?)?); }
    let mut rtt_predictions_ps = Vec::new();
    for obs in rtt { rtt_predictions_ps.push(predict_rtt_ps(distance_mm, clock_bias_ps, obs.path_delay_ps)?); }
    Ok(Calibration { distance_mm, clock_bias_ps, phase_biases_md, pbr_predictions_md, rtt_predictions_ps })
}
