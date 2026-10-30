//! Verifier-owned case inputs.
//!
//! Only the *inputs* live here, because the candidate-tainted probe has to
//! receive them in order to run. Expected results are held exclusively by the
//! `judge` crate, which the candidate never links and cannot read at run time,
//! and the V1 assertion provenance for each case sits there too, above the arm
//! that transcribes it. The per-case notes below say what public behaviour each
//! case makes load-bearing and which family of wrong implementation it rejects,
//! and describe what each fixture is -- never what it should produce.
//!
//! Every fixture is byte-for-byte the V1 fixture for the same case name, rebuilt
//! against the verifier's own contract types. Each case is expressed as a
//! uniform sequence of calls so the probe needs no per-case branching and can
//! stay a pure recorder.

use crate::{fixture, grid, phase, rtt, wrap, PbrObservation, Problem, RttObservation};

/// One call into the candidate API. Uniform across cases.
#[derive(Clone, Debug)]
pub enum Call {
    Recover(Problem),
    Wrap(i128),
    PredictPhase(i64, i64, i64),
    PredictRtt(i64, i64, i64),
}

/// Case population. The judge's `TOTAL` and the reward gate both derive from it.
///
/// The first fifteen entries are the V1 population in the V1 order; the last
/// closes a public-contract gap the V1 fixtures left open, because none of them
/// supplied a named path with an empty grid. Each entry carries the public
/// behaviour it makes load-bearing and the family of wrong implementation it
/// exists to reject. The expected values stay in the judge; nothing below states
/// what any answer is.
pub const CASES: &[&str] = &[
    // Behaviour: distance, clock bias and per-path phase bias are recovered
    // jointly from the wrapped-phase and round-trip streams together.
    // Rejects: fitting one modality only -- a PBR-only or RTT-only solver, and
    // per-observation fitting that never reconciles the two streams globally.
    "joint_global_recovery",
    // Behaviour: phase evidence resolves a distance pair the timing stream
    // cannot separate, because a clock candidate compensates the propagation
    // difference exactly.
    // Rejects: an RTT-only fit, which finds both distances equally good and
    // returns whichever it visited first.
    "pbr_resolves_rtt_clock_ambiguity",
    // Behaviour: timing evidence decides the distance when the phase channel
    // carries no distance slope at all.
    // Rejects: a PBR-only fit, which cannot distinguish the candidates here and
    // so has to guess.
    "rtt_resolves_pbr_ambiguity",
    // Behaviour: candidate grid bounds are inclusive, for the distance, clock
    // and both bias grids.
    // Rejects: exclusive or half-open grid iteration that never reaches the
    // final sorted entry of a grid.
    "bounded_grid_endpoints_included",
    // Behaviour: the calibration depends on the evidence, not on the order the
    // observations and grids were supplied in, and the returned collections are
    // in canonical order rather than input order.
    // Rejects: input-order dependence -- first-match scanning of a grid, or
    // predictions emitted in the order the observations happened to arrive.
    "channel_input_order_is_ignored",
    // Behaviour: with no observations every grid combination scores identically,
    // so the canonical parameter order alone decides, and biases come back in
    // canonical path order regardless of how the paths were supplied.
    // Rejects: first-minimum tie handling, which returns whichever combination
    // the enumeration reached first, and emitting paths in input order.
    "deterministic_empty_evidence_tie",
    // Behaviour: phase error is circular distance on the signed half-open
    // range, so the two representatives of the modulo boundary are close to
    // each other rather than a full turn apart.
    // Rejects: a linear absolute-difference residual, positive-only modulo, and
    // giving the wrong one of -180000/180000 ownership of the boundary; each
    // scores the two bias candidates here in the wrong order.
    "circular_error_drives_selection",
    // Behaviour: the returned predictions are the documented model evaluated at
    // the recovered parameters, not the measurements handed back.
    // Rejects: echoing the measured values as predictions, and truncating
    // instead of rounding, whose drift becomes visible once the wrapped phases
    // sit near the large-magnitude end of the range. Floating point is
    // forbidden by the contract rather than caught here: at Bluetooth
    // frequencies and ranges every intermediate stays exactly representable.
    "predictions_are_load_bearing",
    // Behaviour: each antenna path keeps its own phase bias.
    // Rejects: one global antenna bias shared across paths, which averages an
    // equal-and-opposite pair away to a single value.
    "tied_antenna_paths_remain_distinct",
    // Behaviour: wrap boundaries, RTT/PBR coupling, per-path bias identity and
    // input-order independence all have to hold in the same solution.
    // Rejects: any solver correct on one of those rules but not their
    // conjunction -- positive-only modulo, a shared bias, an RTT-only fit or
    // order-dependent scanning each pick a different calibration here.
    "compound_wrap_rtt_paths_and_order",
    // Behaviour: no grid combination reproduces the evidence exactly, so the
    // complete residual tuple decides: summed phase and timing residuals tie
    // across several parameter choices and only the two maximum components
    // separate them, after which the canonical parameter order breaks a
    // remaining tie. The whole calibration is compared, not its shape.
    // Rejects: comparing only the residual sums; taking the first minimum
    // encountered; assuming the winning maxima are the numerically smallest
    // available; truncating instead of rounding; and mis-owning the modulo
    // boundary, since the sought bias puts one predicted phase exactly on it.
    "compound_joint_maxima_and_boundary_tie",
    // Behaviour: wrapping maps any input onto the signed half-open range, with
    // exactly one of the two boundary magnitudes inside it.
    // Rejects: positive-only modulo, and the off-by-one ownership choice that
    // maps the boundary to the excluded representative.
    "signed_half_open_wrap_boundaries",
    // Behaviour: the round-trip model combines distance, clock bias and path
    // delay in the documented order with exact integer arithmetic.
    // Rejects: applying the terms in a different order, and truncating instead
    // of rounding, both of which shift the result at these inputs. Floating
    // point is forbidden by the contract rather than caught here.
    "exact_rtt_model_order",
    // Behaviour: both predictors fail closed on a negative distance.
    // Rejects: unchecked arithmetic that propagates the negative value into a
    // phase or timing figure instead of refusing it.
    "negative_distance_rejected",
    // Behaviour: an observation naming a path with no bias grid is rejected.
    // Rejects: accepting the missing path by defaulting its bias to zero, which
    // silently invents a calibration for hardware that was never characterised.
    "missing_path_rejected",
    // Behaviour: a named path whose bias grid is present but empty is rejected
    // too, because the contract requires every named path to carry a nonempty
    // grid and an empty grid characterises nothing.
    // Rejects: treating an empty grid as a zero bias, or skipping the path and
    // calibrating from the remaining evidence, both of which answer a problem
    // the contract declares unanswerable.
    "empty_bias_grid_rejected",
];

fn ob(f: i64, path: &str, measured: i64) -> PbrObservation {
    PbrObservation { frequency_khz: f, path_id: path.to_string(), measured_phase_md: measured }
}

fn rb(path: &str, delay: i64, measured: i64) -> RttObservation {
    RttObservation { path_id: path.to_string(), path_delay_ps: delay, measured_rtt_ps: measured }
}

/// Number of recorded values a correct probe run produces for `id`.
/// The judge supplies this to the decoder, so no count travels in the record.
pub fn arity(id: &str) -> Option<usize> {
    plan(id).map(|p| p.len())
}

pub fn plan(id: &str) -> Option<Vec<Call>> {
    let calls = match id {
        // Two-antenna fixture, both evidence streams populated.
        // Behaviour: joint recovery of distance, clock bias and per-path bias.
        // Rejects: a single-modality or per-observation fit.
        "joint_global_recovery" => vec![Call::Recover(fixture(1_350, -120, 179_500, -179_200))],

        // Two distances whose RTT fits coincide under a compensating clock bias;
        // a single PBR observation at one frequency separates them.
        // Behaviour: phase evidence must break a tie the timing stream cannot see.
        // Rejects: an RTT-only fit, which cannot separate these two distances.
        "pbr_resolves_rtt_clock_ambiguity" => {
            let (d0, d1) = (100, 200);
            let alternate_clock = rtt(d1, 0, 0) - rtt(d0, 0, 0);
            let p = Problem {
                distance_candidates_mm: vec![d0, d1],
                clock_bias_candidates_ps: vec![0, alternate_clock],
                phase_bias_candidates_md: grid(&[("p", &[0])]),
                pbr: vec![ob(2_480_000, "p", phase(2_480_000, d1, 0))],
                rtt: vec![rb("p", 0, rtt(d1, 0, 0))],
            };
            // V1 called `recover` twice on this problem; both calls are kept.
            vec![Call::Recover(p.clone()), Call::Recover(p)]
        }

        // A zero-frequency PBR channel, so phase carries no distance slope and
        // the only distance evidence is a single RTT observation.
        // Behaviour: timing evidence decides when phase is uninformative.
        // Rejects: a PBR-only fit, which has no way to choose here.
        "rtt_resolves_pbr_ambiguity" => vec![Call::Recover(Problem {
            distance_candidates_mm: vec![100, 200],
            clock_bias_candidates_ps: vec![0],
            phase_bias_candidates_md: grid(&[("p", &[25])]),
            pbr: vec![ob(0, "p", 25)],
            rtt: vec![rb("p", 0, rtt(200, 0, 0))],
        })],

        // Grids arranged so the sought values sit at the final position of the
        // distance, clock and both bias grids after sorting.
        // Behaviour: candidate bounds are inclusive on every grid.
        // Rejects: exclusive or half-open grid iteration, which never evaluates
        // the last sorted entry and so cannot reach the answer.
        "bounded_grid_endpoints_included" => {
            let mut p = fixture(777, 45, -175_000, 176_000);
            p.distance_candidates_mm = vec![775, 776, 777];
            p.clock_bias_candidates_ps = vec![43, 44, 45];
            p.phase_bias_candidates_md.get_mut("ant-a").unwrap().sort();
            p.phase_bias_candidates_md.get_mut("ant-b").unwrap().sort();
            vec![Call::Recover(p)]
        }

        // The same problem twice: once as built, once with every observation
        // list and every candidate grid reversed.
        // Behaviour: both calls must agree, and the returned collections are in
        // canonical order rather than the order supplied.
        // Rejects: input-order dependence -- first-match grid scanning, or
        // predictions emitted in arrival order.
        "channel_input_order_is_ignored" => {
            let p = fixture(2_005, 31, 8_000, -11_000);
            let mut q = p.clone();
            q.pbr.reverse();
            q.rtt.reverse();
            q.distance_candidates_mm.reverse();
            q.clock_bias_candidates_ps.reverse();
            for values in q.phase_bias_candidates_md.values_mut() {
                values.reverse();
            }
            vec![Call::Recover(p), Call::Recover(q)]
        }

        // No observations at all, so every grid combination scores identically
        // and only the canonical tie rule separates them. Two paths, supplied in
        // non-alphabetical order, each with a symmetric bias grid.
        // Behaviour: the canonical parameter order is the whole decision, and
        // biases come back in canonical path order.
        // Rejects: first-minimum tie handling, and emitting paths as supplied.
        "deterministic_empty_evidence_tie" => vec![Call::Recover(Problem {
            distance_candidates_mm: vec![5, 3],
            clock_bias_candidates_ps: vec![2, -2],
            phase_bias_candidates_md: grid(&[("z", &[8, -8]), ("a", &[9, -9])]),
            pbr: vec![],
            rtt: vec![],
        })],

        // A measured phase sitting exactly on the modulo boundary, with two bias
        // candidates on opposite signed representatives of it.
        // Behaviour: phase error is circular distance on the signed half-open
        // range, so the two boundary representatives are near each other.
        // Rejects: a linear absolute-difference residual, positive-only modulo,
        // and the wrong -180000/180000 boundary ownership.
        "circular_error_drives_selection" => vec![Call::Recover(Problem {
            distance_candidates_mm: vec![0],
            clock_bias_candidates_ps: vec![0],
            phase_bias_candidates_md: grid(&[("p", &[179_999, -179_000])]),
            pbr: vec![ob(2_402_000, "p", -180_000)],
            rtt: vec![],
        })],

        // Two-antenna fixture whose biases put several channel phases near the
        // large-magnitude end of the wrapped range.
        // Behaviour: predictions are the documented model at the recovered
        // parameters, evaluated exactly.
        // Rejects: echoing the measurements back as predictions, and
        // truncating instead of rounding, whose drift shows at these
        // magnitudes. Floating point is forbidden by the contract rather than
        // caught here: every intermediate stays exactly representable.
        "predictions_are_load_bearing" => vec![Call::Recover(fixture(1_111, -88, 170_000, -160_000))],

        // Two antenna paths carrying equal and opposite biases.
        // Behaviour: per-path bias identity is preserved.
        // Rejects: one global antenna bias, which averages the pair to zero.
        "tied_antenna_paths_remain_distinct" => vec![Call::Recover(fixture(930, 0, 70_000, -70_000))],

        // Compound fixture: biases at both ends of the wrapped range, both
        // observation lists rotated out of their natural order.
        // Behaviour: wrap ownership, RTT/PBR coupling, per-path bias and
        // order independence hold together in one solution.
        // Rejects: positive-only modulo, a shared bias, an RTT-only fit, or
        // order-dependent scanning -- each picks a different calibration.
        "compound_wrap_rtt_paths_and_order" => {
            let mut p = fixture(2_499, 123, 179_999, -179_999);
            p.pbr.rotate_left(1);
            p.rtt.reverse();
            vec![Call::Recover(p)]
        }

        // Joint compound fixture. Two antenna paths carry two channels each and
        // both also appear in the RTT stream. Every bias grid and both
        // observation lists are supplied out of canonical order, one nearer
        // distance carries a compensating clock candidate, and no combination in
        // the grids reproduces the evidence exactly.
        // Behaviour: with no exact fit available the full residual tuple
        // decides -- the summed components tie, the two maxima separate the
        // survivors, and the canonical parameter order settles what remains.
        // Rejects: comparing only the residual sums; taking the first minimum
        // reached; assuming the winning maxima are the smallest available;
        // truncating instead of rounding; and mis-owning the modulo boundary,
        // which one predicted phase lands exactly on.
        "compound_joint_maxima_and_boundary_tie" => {
            const F_A1: i64 = 2_402_000;
            const F_A2: i64 = 2_480_000;
            const F_B1: i64 = 2_426_000;
            const F_B2: i64 = 2_442_000;
            let (d, near, far, clock) = (1_200_i64, 950_i64, 1_337_i64, 61_i64);
            // Anchor the first ant-a measurement so that the sought bias predicts
            // the phase sitting exactly on the modulo boundary.
            let anchor = 179_980_i64;
            let r = wrap(anchor as i128 - phase(F_A1, d, 0) as i128);
            let ga = [r + 80, r + 20];
            let gb = [150_i64, 0, 100];
            let base = rtt(d, clock, 0) - 200;
            let mut pbr = vec![
                ob(F_A1, "ant-a", anchor),
                ob(F_A2, "ant-a", phase(F_A2, d, r + 100)),
                ob(F_B1, "ant-b", phase(F_B1, d, 0)),
                ob(F_B2, "ant-b", phase(F_B2, d, 200)),
            ];
            pbr.rotate_left(1);
            vec![Call::Recover(Problem {
                distance_candidates_mm: vec![near, far, d],
                clock_bias_candidates_ps: vec![
                    clock + rtt(d, 0, 0) - rtt(near, 0, 0),
                    clock - 200,
                    clock,
                ],
                phase_bias_candidates_md: grid(&[("ant-a", &ga), ("ant-b", &gb)]),
                pbr,
                rtt: vec![rb("ant-b", 33, base + 33), rb("ant-a", -17, base + 400 - 17)],
            })]
        }

        // Direct wrap calls: zero, both sides of the modulo boundary, a value one
        // below the negative boundary, and a value a full turn and a half up.
        // Behaviour: wrapping lands in the signed half-open range, with exactly
        // one of the two boundary magnitudes inside it.
        // Rejects: positive-only modulo, and the off-by-one ownership that maps
        // the boundary onto the excluded representative.
        "signed_half_open_wrap_boundaries" => vec![
            Call::Wrap(0),
            Call::Wrap(179_999),
            Call::Wrap(180_000),
            Call::Wrap(-180_001),
            Call::Wrap(540_000),
        ],

        // Direct RTT calls: one at zero distance with a negative clock bias and a
        // positive path delay, one at a round distance with neither.
        // Behaviour: the timing model combines distance, clock bias and path
        // delay in the documented order with exact integer arithmetic.
        // Rejects: reordering the terms, and truncating instead of rounding,
        // both of which move the result at exactly these inputs. Floating point
        // is forbidden by the contract rather than caught here.
        "exact_rtt_model_order" => vec![Call::PredictRtt(0, -9, 14), Call::PredictRtt(1_000, 0, 0)],

        // Both predictors called with a negative distance.
        // Behaviour: both predictors fail closed rather than returning a figure.
        // Rejects: unchecked arithmetic that propagates the negative distance
        // into a phase or timing value.
        "negative_distance_rejected" => {
            vec![Call::PredictPhase(2_402_000, -1, 0), Call::PredictRtt(-1, 0, 0)]
        }

        // A PBR observation naming a path that has no bias grid at all.
        // Behaviour: an uncharacterised antenna path is rejected.
        // Rejects: accepting the missing path by defaulting its bias to zero,
        // which invents a calibration for hardware never characterised.
        "missing_path_rejected" => vec![Call::Recover(Problem {
            distance_candidates_mm: vec![1],
            clock_bias_candidates_ps: vec![0],
            phase_bias_candidates_md: Default::default(),
            pbr: vec![ob(2_402_000, "x", 0)],
            rtt: vec![],
        })],

        // A PBR observation whose path is present in the grid map but carries no
        // candidate values at all. Both other grids are populated, so nothing
        // else can be the ground for rejection.
        // Behaviour: a named path must carry a nonempty bias grid.
        // Rejects: reading an empty grid as a zero bias, and skipping the path to
        // calibrate from the remaining evidence.
        "empty_bias_grid_rejected" => vec![Call::Recover(Problem {
            distance_candidates_mm: vec![1],
            clock_bias_candidates_ps: vec![0],
            phase_bias_candidates_md: grid(&[("p", &[])]),
            pbr: vec![ob(2_402_000, "p", 0)],
            rtt: vec![],
        })],

        _ => return None,
    };
    Some(calls)
}
