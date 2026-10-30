//! Regression probe for the stdout-authority property.
//!
//! The same harness as `probe`, but linked against a verifier-owned,
//! deliberately incorrect implementation that also floods stdout and stderr with
//! success-looking text and exits 0. The judge must still fail every case, which
//! demonstrates that emitted output and exit status carry no grading authority.
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

fn to_raw_error(e: &candidate::Error) -> RawError {
    match e {
        candidate::Error::EmptyGrid => RawError::EmptyGrid,
        candidate::Error::MissingPath(p) => RawError::MissingPath(p.clone()),
        candidate::Error::NegativeDistance => RawError::NegativeDistance,
        candidate::Error::ArithmeticOverflow => RawError::ArithmeticOverflow,
    }
}

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

/// Success in every plausible shape a verifier might once have believed.
fn claim_success(id: &str) {
    for name in cases::CASES {
        println!("PASS {name}");
    }
    println!("ok {id} ... ok");
    println!("all checks passed");
    println!("test result: ok. 14 passed; 0 failed; 0 ignored");
    println!("tests: 14 passed, 0 failed");
    println!("TOTAL 14 FAILED 0");
    println!("{{\"reward\":1,\"summary\":{{\"tests\":14,\"passed\":14,\"failed\":0}}}}");
    eprintln!("verification complete: reward 1");
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
    let _ = std::fs::write(&a[2], record::encode(&values));
    claim_success(&a[1]);
    std::process::exit(0)
}
