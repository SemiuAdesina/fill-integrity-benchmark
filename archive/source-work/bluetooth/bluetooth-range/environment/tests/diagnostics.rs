
//! Candidate-visible synthetic checks for `spec/channel-model.md`.

use bluetooth_range::{
    predict_phase_md, predict_rtt_ps, recover, wrap_phase_md, Error, PbrObservation, Problem,
    RttObservation, LIGHT_SPEED,
};
use std::collections::BTreeMap;

fn grid(entries: &[(&str, &[i64])]) -> BTreeMap<String, Vec<i64>> {
    entries.iter().map(|(p, v)| (p.to_string(), v.to_vec())).collect()
}

fn model_wrap(value: i128) -> i64 {
    let positive = value.rem_euclid(360_000);
    (if positive >= 180_000 { positive - 360_000 } else { positive }) as i64
}

fn model_phase(frequency: i64, distance: i64, bias: i64) -> i64 {
    let n = 720_000_i128 * frequency as i128 * distance as i128;
    model_wrap((n + LIGHT_SPEED / 2) / LIGHT_SPEED + bias as i128)
}

fn model_rtt(distance: i64, clock: i64, delay: i64) -> i64 {
    let n = 2_000_000_000_i128 * distance as i128;
    ((n + LIGHT_SPEED / 2) / LIGHT_SPEED) as i64 + clock + delay
}

#[test]
fn signed_wrap_owns_negative_half_turn() {
    assert_eq!(wrap_phase_md(180_000), Ok(-180_000),
        "FAILED: phase wrap owns -180000, not +180000");
    assert_eq!(wrap_phase_md(-180_001), Ok(179_999));
}

#[test]
fn rtt_rounds_propagation_before_adding_offsets() {
    assert_eq!(predict_rtt_ps(1, -3, 4), Ok(8),
        "FAILED: round propagation RTT first, then add clock and path offsets");
}

#[test]
fn large_integer_phase_rounding_stays_exact() {
    assert_eq!(
        predict_phase_md(2_402_000, 1_000_000_000_001, 0),
        Ok(120_690),
        "FAILED: keep phase propagation in exact integer arithmetic"
    );
}

#[test]
fn pbr_breaks_an_rtt_distance_clock_ambiguity() {
    let (near, far) = (120, 240);
    let compensating = model_rtt(far, 0, 0) - model_rtt(near, 0, 0);
    let problem = Problem {
        distance_candidates_mm: vec![near, far],
        clock_bias_candidates_ps: vec![compensating, 0],
        phase_bias_candidates_md: grid(&[("p", &[0])]),
        pbr: vec![PbrObservation { frequency_khz: 2_414_000, path_id: "p".into(),
            measured_phase_md: model_phase(2_414_000, far, 0) }],
        rtt: vec![RttObservation { path_id: "p".into(), path_delay_ps: 0,
            measured_rtt_ps: model_rtt(far, 0, 0) }],
    };
    let answer = recover(&problem).expect("synthetic problem is valid");
    assert_eq!((answer.distance_mm, answer.clock_bias_ps), (far, 0),
        "FAILED: fit PBR and RTT jointly; RTT alone leaves this pair ambiguous");
}

#[test]
fn antenna_paths_keep_independent_biases() {
    let problem = Problem {
        distance_candidates_mm: vec![0], clock_bias_candidates_ps: vec![0],
        phase_bias_candidates_md: grid(&[("left", &[0, 40]), ("right", &[0, -40])]),
        pbr: vec![
            PbrObservation { frequency_khz: 0, path_id: "left".into(), measured_phase_md: 40 },
            PbrObservation { frequency_khz: 0, path_id: "right".into(), measured_phase_md: -40 },
        ], rtt: vec![],
    };
    let answer = recover(&problem).expect("synthetic problem is valid");
    assert_eq!(answer.phase_biases_md,
        vec![("left".to_string(), 40), ("right".to_string(), -40)],
        "FAILED: recover one independent phase bias per named antenna path");
}

#[test]
fn maximum_residual_breaks_a_sum_tie() {
    let problem = Problem {
        distance_candidates_mm: vec![0], clock_bias_candidates_ps: vec![0],
        phase_bias_candidates_md: grid(&[("p", &[0, 10])]),
        pbr: vec![
            PbrObservation { frequency_khz: 0, path_id: "p".into(), measured_phase_md: 0 },
            PbrObservation { frequency_khz: 0, path_id: "p".into(), measured_phase_md: 20 },
        ], rtt: vec![],
    };
    let answer = recover(&problem).expect("synthetic problem is valid");
    assert_eq!(answer.phase_biases_md[0].1, 10,
        "FAILED: compare the complete residual tuple, including the maximum");
}

#[test]
fn empty_evidence_uses_the_canonical_parameter_tie() {
    let problem = Problem {
        distance_candidates_mm: vec![7, 3], clock_bias_candidates_ps: vec![4, -4],
        phase_bias_candidates_md: grid(&[("z", &[9, -9]), ("a", &[8, -8])]),
        pbr: vec![], rtt: vec![],
    };
    let answer = recover(&problem).expect("synthetic problem is valid");
    assert_eq!((answer.distance_mm, answer.clock_bias_ps), (3, -4));
    assert_eq!(answer.phase_biases_md,
        vec![("a".to_string(), -8), ("z".to_string(), -9)],
        "FAILED: equal scores use canonical parameter order, not visit order");
}

#[test]
fn predictions_are_model_derived_and_canonically_ordered() {
    let problem = Problem {
        distance_candidates_mm: vec![100], clock_bias_candidates_ps: vec![2],
        phase_bias_candidates_md: grid(&[("z", &[5]), ("a", &[-7])]),
        pbr: vec![
            PbrObservation { frequency_khz: 2_480_000, path_id: "z".into(), measured_phase_md: 123 },
            PbrObservation { frequency_khz: 2_402_000, path_id: "a".into(), measured_phase_md: -456 },
        ],
        rtt: vec![
            RttObservation { path_id: "z".into(), path_delay_ps: 5, measured_rtt_ps: 0 },
            RttObservation { path_id: "a".into(), path_delay_ps: -3, measured_rtt_ps: 0 },
        ],
    };
    let answer = recover(&problem).expect("synthetic problem is valid");
    assert_eq!(answer.pbr_predictions_md,
        vec![model_phase(2_402_000, 100, -7), model_phase(2_480_000, 100, 5)]);
    assert_eq!(answer.rtt_predictions_ps,
        vec![model_rtt(100, 2, -3), model_rtt(100, 2, 5)],
        "FAILED: predictions follow canonical observation order");
    assert_eq!(predict_phase_md(2_402_000, 100, -7), Ok(answer.pbr_predictions_md[0]));
}

#[test]
fn invalid_distance_and_path_grids_fail_closed() {
    assert_eq!(predict_phase_md(1, -1, 0), Err(Error::NegativeDistance));
    assert_eq!(predict_rtt_ps(-1, 0, 0), Err(Error::NegativeDistance));
    let problem = Problem {
        distance_candidates_mm: vec![0], clock_bias_candidates_ps: vec![0],
        phase_bias_candidates_md: grid(&[("named", &[])]),
        pbr: vec![PbrObservation { frequency_khz: 0, path_id: "named".into(), measured_phase_md: 0 }],
        rtt: vec![],
    };
    assert_eq!(recover(&problem), Err(Error::EmptyGrid));
}
