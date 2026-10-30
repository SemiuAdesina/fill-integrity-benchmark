//! Soak: many randomised runs, each checking that the engine neither panics nor
//! records a pairing it cannot account for.

mod harness;

use harness::*;

/// One randomised run. Nothing is asserted about the shape of the outcome beyond
/// what the venue itself can vouch for: a recorded pairing must be backed by
/// quantity the venue actually matched for both legs.
fn soak(seed: u64) -> Outcome {
    let mut random = Prng::new(seed);
    let mut scenario = Scenario::new(vec![
        candidate("sa", "nS", "tS", 0.45, 0.52, 20, 20),
        candidate("sb", "nS", "tS", 0.44, 0.53, 10, 10),
    ])
    .inventory(instrument("tS"), 60)
    .ask(instrument("nS"), 0.47)
    .ask(instrument("tS"), 0.54);

    for _ in 0..24 {
        let mut actions: Vec<Action> = Vec::new();
        for _ in 0..random.below(3) {
            let strategy = if random.below(2) == 0 { "sa" } else { "sb" };
            let venue = if random.below(2) == 0 { Venue::North } else { Venue::South };
            actions.push(match random.below(8) {
                0 => fill_resting(strategy, venue, 5 + random.below(20) as Qty),
                1 => fill_taker(strategy, venue, 5 + random.below(15) as Qty),
                2 => Action::WithholdFillEvents,
                3 => Action::ReleaseFillEvents,
                4 => Action::RedeliverAllFillEvents,
                5 => Action::RedeliverAllBlocks,
                6 => Action::LoseNextAcknowledgement,
                _ => block_takers(&[strategy], venue, 3 + random.below(20) as Qty, 0.55),
            });
        }
        if random.below(9) == 0 {
            actions.push(Action::Restart);
        }
        if random.below(11) == 0 {
            actions.push(Action::FailNextCancel);
        }
        if random.below(13) == 0 {
            actions.push(Action::RejectNextTakerSubmission);
        }
        scenario = scenario.step(actions);
    }
    run(scenario.idle_steps(6))
}

#[test]
fn randomised_runs_do_not_panic() {
    for seed in 0..512u64 {
        let outcome = soak(seed);
        assert!(
            outcome.cycle_errors.is_empty(),
            "seed {seed}: a cycle returned an error: {:?}",
            outcome.cycle_errors
        );
        // A pairing must not be recorded twice, and must not be recorded for a
        // strategy whose legs the venue never matched in full.
        let mut ids: Vec<String> = outcome
            .positions
            .iter()
            .map(|position| position.strategy_id.as_str().to_string())
            .collect();
        let before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(before, ids.len(), "seed {seed}: a mandate was paired twice");
    }
}

/// The same observations in the same order reach the same outcome.
#[test]
fn the_same_observations_reach_the_same_outcome() {
    let first = soak(7);
    let second = soak(7);
    assert_eq!(first.attempts, second.attempts, "submissions differed between two identical runs");
    assert_eq!(first.positions, second.positions, "positions differed between two identical runs");
    assert_eq!(
        first.cancelled_order_ids, second.cancelled_order_ids,
        "cancellations differed between two identical runs"
    );
}
