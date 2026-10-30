//! Verifier-owned probe.
//!
//! Links exactly one allowlisted candidate file (`/app/src/lib.rs`) as the
//! `candidate` module and records the raw functional results of the calls that
//! one verifier-named case makes.
//!
//! The probe deliberately has no grading role. It asserts nothing, compares
//! nothing, prints no verdict and emits no marker, and its exit status is logged
//! but never consulted. Grading happens afterwards in the separate `judge`
//! binary, which never links candidate code. Anything this process writes to
//! stdout or stderr is diagnostics only.
//!
//! Note the translation across the trust boundary: case inputs are built from
//! the verifier's own frozen `bt_model` types and converted into the candidate's
//! declared types on the way in, and the candidate's returned values are
//! converted straight back into the verifier's raw record shape on the way out.
//! The candidate never supplies a type, a comparison or a conclusion.
#[allow(dead_code)]
mod candidate;

use bt_model::{cases, cases::Call, record, RawCalibration, RawError, Value};

/// Verifier input -> candidate input. Pure field translation.
fn to_candidate(p: &bt_model::Problem) -> candidate::Problem {
    candidate::Problem {
        distance_candidates_mm: p.distance_candidates_mm.clone(),
        clock_bias_candidates_ps: p.clock_bias_candidates_ps.clone(),
        phase_bias_candidates_md: p
            .phase_bias_candidates_md
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        pbr: p
            .pbr
            .iter()
            .map(|o| candidate::PbrObservation {
                frequency_khz: o.frequency_khz,
                path_id: o.path_id.clone(),
                measured_phase_md: o.measured_phase_md,
            })
            .collect(),
        rtt: p
            .rtt
            .iter()
            .map(|o| candidate::RttObservation {
                path_id: o.path_id.clone(),
                path_delay_ps: o.path_delay_ps,
                measured_rtt_ps: o.measured_rtt_ps,
            })
            .collect(),
    }
}

/// Candidate error -> raw record error. Pure translation.
fn to_raw_error(e: &candidate::Error) -> RawError {
    match e {
        candidate::Error::EmptyGrid => RawError::EmptyGrid,
        candidate::Error::MissingPath(p) => RawError::MissingPath(p.clone()),
        candidate::Error::NegativeDistance => RawError::NegativeDistance,
        candidate::Error::ArithmeticOverflow => RawError::ArithmeticOverflow,
    }
}

/// Candidate calibration -> raw record calibration. Nothing is judged,
/// normalised, sorted, repaired or rejected here.
fn to_raw_cal(c: &candidate::Calibration) -> RawCalibration {
    RawCalibration {
        distance_mm: c.distance_mm,
        clock_bias_ps: c.clock_bias_ps,
        phase_biases_md: c.phase_biases_md.clone(),
        pbr_predictions_md: c.pbr_predictions_md.clone(),
        rtt_predictions_ps: c.rtt_predictions_ps.clone(),
    }
}

fn int_or_err(r: Result<i64, candidate::Error>) -> Value {
    match r {
        Ok(v) => Value::Int(v),
        Err(e) => Value::Err(to_raw_error(&e)),
    }
}

fn run(call: &Call) -> Value {
    match call {
        Call::Recover(p) => match candidate::recover(&to_candidate(p)) {
            Ok(c) => Value::Cal(to_raw_cal(&c)),
            Err(e) => Value::Err(to_raw_error(&e)),
        },
        Call::Wrap(v) => int_or_err(candidate::wrap_phase_md(*v)),
        Call::PredictPhase(f, d, b) => int_or_err(candidate::predict_phase_md(*f, *d, *b)),
        Call::PredictRtt(d, c, p) => int_or_err(candidate::predict_rtt_ps(*d, *c, *p)),
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 3 {
        std::process::exit(2)
    }
    let Some(plan) = cases::plan(&a[1]) else {
        std::process::exit(2)
    };
    let values: Vec<Value> = plan.iter().map(run).collect();
    if std::fs::write(&a[2], record::encode(&values)).is_err() {
        std::process::exit(2)
    }
}
