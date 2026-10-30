//! Verifier-owned frozen model for the `bluetooth-range` task.
//!
//! This crate holds the verifier's OWN copies of the data contract types, the
//! frozen integer channel model, the case inputs and the raw record codec. It
//! contains no recovery search, so it can be linked into both the tainted probe
//! and the untainted judge without giving candidate code any influence over the
//! judge.
//!
//! The types below are deliberately independent of the candidate's declarations.
//! The probe translates field-by-field in both directions; the candidate never
//! supplies a type, a comparison or a conclusion.

pub mod cases;
pub mod record;

use std::collections::BTreeMap;

/// Speed of light in mm/s as used by the frozen channel model.
pub const LIGHT_SPEED: i128 = 299_792_458;
pub const FULL_PHASE_MD: i128 = 360_000;
pub const HALF_PHASE_MD: i128 = 180_000;

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

/// Raw transport shape of a candidate `Calibration`. No verdict lives here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawCalibration {
    pub distance_mm: i64,
    pub clock_bias_ps: i64,
    pub phase_biases_md: Vec<(String, i64)>,
    pub pbr_predictions_md: Vec<i64>,
    pub rtt_predictions_ps: Vec<i64>,
}

/// Raw transport shape of a candidate `Error`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawError {
    EmptyGrid,
    MissingPath(String),
    NegativeDistance,
    ArithmeticOverflow,
}

/// One recorded functional value. There is no status variant, no pass/fail flag
/// and no marker: the codec simply cannot express a verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Err(RawError),
    Cal(RawCalibration),
}

/// Frozen rational rounding: round-half-up for positive denominators.
pub fn round_ratio(n: i128, d: i128) -> i128 {
    (n + d / 2) / d
}

/// Frozen signed half-open wrap into `[-180000, 180000)`.
pub fn wrap(v: i128) -> i64 {
    let x = v.rem_euclid(FULL_PHASE_MD);
    (if x >= HALF_PHASE_MD { x - FULL_PHASE_MD } else { x }) as i64
}

/// Frozen PBR phase model, in millidegrees.
pub fn phase(f: i64, d: i64, b: i64) -> i64 {
    wrap(round_ratio(720_000_i128 * f as i128 * d as i128, LIGHT_SPEED) + b as i128)
}

/// Frozen RTT model, in picoseconds.
pub fn rtt(d: i64, c: i64, p: i64) -> i64 {
    (round_ratio(2_000_000_000_i128 * d as i128, LIGHT_SPEED) + c as i128 + p as i128) as i64
}

/// The V1 two-antenna fixture builder, carried over unchanged.
pub fn fixture(distance: i64, clock: i64, a: i64, b: i64) -> Problem {
    let mut grids = BTreeMap::new();
    grids.insert("ant-a".to_string(), vec![a - 1_000, a, a + 1_000]);
    grids.insert("ant-b".to_string(), vec![b + 900, b, b - 900]);
    let mut pbr = Vec::new();
    for (f, path, bias) in [
        (2_402_000, "ant-a", a),
        (2_426_000, "ant-b", b),
        (2_480_000, "ant-a", a),
        (2_442_000, "ant-b", b),
    ] {
        pbr.push(PbrObservation {
            frequency_khz: f,
            path_id: path.to_string(),
            measured_phase_md: phase(f, distance, bias),
        });
    }
    let rtt_obs = vec![
        RttObservation {
            path_id: "ant-b".to_string(),
            path_delay_ps: 33,
            measured_rtt_ps: rtt(distance, clock, 33),
        },
        RttObservation {
            path_id: "ant-a".to_string(),
            path_delay_ps: -17,
            measured_rtt_ps: rtt(distance, clock, -17),
        },
    ];
    Problem {
        distance_candidates_mm: vec![distance + 1, distance - 1, distance],
        clock_bias_candidates_ps: vec![clock + 7, clock, clock - 7],
        phase_bias_candidates_md: grids,
        pbr,
        rtt: rtt_obs,
    }
}

/// Convenience constructor for the small hand-built fixtures.
pub fn grid(pairs: &[(&str, &[i64])]) -> BTreeMap<String, Vec<i64>> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_vec()))
        .collect()
}
