
//! Public contract diagnostics.
//!
//! These checks exercise the supplied model, transition, canonical digest and
//! result-format helpers. They intentionally do not exercise or reveal the
//! optimiser that the task asks the agent to implement.

use polyploid_phase::cross_model::{
    Haplotypes, ProgenyPath, Reconstruction, State, ALL_STATES, STATES,
};
use polyploid_phase::digest::{canonical_digest, serialise_for_digest};
use polyploid_phase::result_output::{decode, encode};

fn tiny_reconstruction() -> Reconstruction {
    let haplotypes_a = Haplotypes::new([
        vec![0, 1],
        vec![1, 0],
        vec![0, 0],
        vec![1, 1],
    ]);
    let haplotypes_b = Haplotypes::new([
        vec![1, 0],
        vec![0, 1],
        vec![1, 1],
        vec![0, 0],
    ]);
    Reconstruction {
        haplotypes_a,
        haplotypes_b,
        paths: vec![ProgenyPath {
            from_a: vec![State::new(2, 1).unwrap(), State::new(3, 3).unwrap()],
            from_b: vec![State::new(0, 2).unwrap(), State::new(2, 3).unwrap()],
        }],
    }
}

#[test]
fn inheritance_states_are_unordered_and_complete() {
    assert_eq!(STATES, 10);
    assert_eq!(ALL_STATES.len(), STATES);
    assert_eq!(State::new(2, 1), State::new(1, 2));
    assert!(State::new(4, 0).is_none());
    for (index, state) in ALL_STATES.iter().enumerate() {
        assert!(state.low() <= state.high());
        assert_eq!(state.canonical_index(), index);
    }
}

#[test]
fn switch_counts_use_multiset_semantics() {
    let repeated = State::new(1, 1).unwrap();
    assert!(repeated.is_double_reduction());
    assert_eq!(repeated.switch_count(State::new(1, 2).unwrap()), 1);
    assert_eq!(repeated.switch_count(State::new(2, 3).unwrap()), 2);
}

#[test]
fn relabelling_resorts_the_unordered_pair() {
    let state = State::new(0, 3).unwrap();
    let permutation = [3, 1, 2, 0];
    assert_eq!(state.remap(&permutation), State::new(0, 3).unwrap());
}

#[test]
fn result_encoding_round_trips_and_digest_is_deterministic() {
    let answer = tiny_reconstruction();
    let first = canonical_digest(&answer);
    let second = canonical_digest(&answer);
    assert_eq!(first, second);
    assert_eq!(first.len(), 16);
    assert_eq!(serialise_for_digest(&answer).last(), Some(&b'#'));

    let text = encode(&answer, -12.5);
    let decoded = decode(&text, 2, 1).expect("supplied encoder must emit valid result JSON");
    assert_eq!(decoded.answer, answer);
    assert_eq!(decoded.total_log_likelihood, -12.5);
    assert_eq!(decoded.canonical_digest, first);
}
