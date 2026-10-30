//! Verifier-owned wrong-by-construction implementation used only by the
//! stdout-authority regression. Every value it returns is structurally outside
//! the legal domain, so it cannot accidentally coincide with a correct answer:
//!
//! * `wrap_phase_md` returns a value at least 720000, never inside the required
//!   half-open interval `[-180000, 180000)`;
//! * `predict_phase_md` always succeeds with 360000, so it never reports the
//!   required `NegativeDistance`;
//! * `predict_rtt_ps` always reports `ArithmeticOverflow`;
//! * `recover` always succeeds, choosing one past the maximum of every supplied
//!   grid (a value no grid contains), and emits predictions in raw input order
//!   offset by a full turn, which is both order-sensitive and outside the legal
//!   wrapped phase interval.
use std::collections::BTreeMap;

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
    Ok(value.rem_euclid(360_000) as i64 + 720_000)
}

pub fn predict_phase_md(_f: i64, _d: i64, _b: i64) -> Result<i64, Error> {
    Ok(360_000)
}

pub fn predict_rtt_ps(_d: i64, _c: i64, _p: i64) -> Result<i64, Error> {
    Err(Error::ArithmeticOverflow)
}

fn past_max(values: &[i64]) -> i64 {
    values.iter().copied().max().unwrap_or(0).saturating_add(1)
}

pub fn recover(problem: &Problem) -> Result<Calibration, Error> {
    Ok(Calibration {
        distance_mm: past_max(&problem.distance_candidates_mm),
        clock_bias_ps: past_max(&problem.clock_bias_candidates_ps),
        phase_biases_md: problem
            .phase_bias_candidates_md
            .iter()
            .map(|(name, grid)| (name.clone(), past_max(grid)))
            .collect(),
        pbr_predictions_md: problem
            .pbr
            .iter()
            .map(|o| o.measured_phase_md + 360_000)
            .collect(),
        rtt_predictions_ps: problem
            .rtt
            .iter()
            .map(|o| o.measured_rtt_ps + 360_000)
            .collect(),
    })
}
