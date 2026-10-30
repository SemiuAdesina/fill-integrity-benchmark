//! Diagnostics: seeded interleavings of fills, redeliveries, lost acknowledgements
//! and restarts across three mandates.
//!
//! Each seed permutes the order in which venue fills, inventory freezes, withheld
//! and redelivered fill events, lost acknowledgements and restarts reach the
//! engine. No arrival order is privileged. Every seed is checked against the
//! safety properties of `/app/spec/reconciliation-contract.md`: one position at
//! most per mandate, no venue acquiring more than the mandate intended, no
//! corrective order sent twice under one client reference, and a completed
//! mandate covered on both venues. A failure names the seed and the observable
//! outcome that was wrong, never the cause. None of this is graded.

mod harness;

use harness::*;

fn position_count(outcome: &Outcome, strategy_id: &str) -> usize {
    outcome
        .positions
        .iter()
        .filter(|position| position.strategy_id.as_str() == strategy_id)
        .count()
}

fn intended(outcome: &Outcome, strategy_id: &str, venue: Venue) -> Qty {
    outcome
        .resting_for(strategy_id, venue)
        .into_iter()
        .map(|order| order.size)
        .next()
        .unwrap_or(0)
}

fn acquired(outcome: &Outcome, strategy_id: &str, venue: Venue) -> Qty {
    outcome
        .venue_orders
        .iter()
        .filter(|order| order.strategy_id.as_str() == strategy_id && order.venue == venue)
        .map(|order| order.filled_qty)
        .sum()
}

/// Three mandates; the third shares its south-side instrument with the first on
/// half of the seeds, so identifiers seen on that instrument may belong to
/// either. Fills, freezes, withholds, redeliveries, lost acknowledgements and
/// restarts land in a seed-dependent order over twelve cycles, then the world
/// settles.
fn interleaving(seed: u64) -> Scenario {
    let mut prng = Prng::new(seed);
    let shared = prng.below(2) == 0;
    let third_south = if shared { "iA" } else { "iY" };

    let mut scenario = Scenario::new(vec![
        candidate("oa", "iA", "iB", 0.45, 0.52, 30, 30),
        candidate("ob", "iX", "iC", 0.44, 0.53, 20, 20),
        candidate("oc", third_south, "iD", 0.46, 0.51, 12, 12),
    ])
    .inventory(instrument("iB"), (prng.below(3) * 10) as Qty)
    .ask(instrument("iA"), 0.47)
    .ask(instrument("iB"), 0.60)
    .ask(instrument("iC"), 0.61)
    .ask(instrument("iD"), 0.59)
    .ask(instrument("iX"), 0.48)
    .ask(instrument("iY"), 0.49)
    .step(vec![]);

    let quantities = [30, 20, 12, 8, 5];
    let mandates = ["oa", "ob", "oc"];
    for index in 0..12 {
        let mut actions = Vec::new();
        match prng.below(8) {
            0 | 1 => {
                let mandate = mandates[prng.below(3)];
                actions.push(fill_resting(mandate, Venue::North, quantities[prng.below(5)]));
            }
            2 | 3 => {
                let mandate = mandates[prng.below(3)];
                actions.push(fill_resting(mandate, Venue::South, quantities[prng.below(5)]));
            }
            4 => actions.push(if index % 2 == 0 {
                Action::FreezeInventory
            } else {
                Action::UnfreezeInventory
            }),
            5 => actions.push(if index % 2 == 0 {
                Action::WithholdFillEvents
            } else {
                Action::ReleaseFillEvents
            }),
            6 => actions.push(Action::RedeliverAllFillEvents),
            _ => {
                let mandate = mandates[prng.below(3)];
                actions.push(fill_resting(mandate, Venue::North, quantities[prng.below(5)]));
                actions.push(Action::RedeliverAllFillEvents);
            }
        }
        if prng.below(4) == 0 {
            actions.push(Action::LoseNextAcknowledgement);
        }
        if prng.below(5) == 0 {
            actions.push(Action::Restart);
        }
        scenario = scenario.step(actions);
    }

    scenario
        .step(vec![Action::UnfreezeInventory, Action::ReleaseFillEvents])
        .idle_steps(8)
}

/// The properties every seed must hold, reported in the contract's terms.
fn check_seed(seed: u64) {
    let outcome = run(interleaving(seed));
    for strategy_id in ["oa", "ob", "oc"] {
        let positions = position_count(&outcome, strategy_id);
        assert!(
            positions <= 1,
            "seed {}: mandate {} holds {} positions; a mandate is at most one position",
            seed,
            strategy_id,
            positions
        );
        for venue in [Venue::North, Venue::South] {
            let intended_size = intended(&outcome, strategy_id, venue);
            if intended_size <= 0 {
                continue;
            }
            let acquired_size = acquired(&outcome, strategy_id, venue);
            assert!(
                acquired_size <= intended_size,
                "seed {}: mandate {} acquired {} on {:?} against an intended size of {}; \
                 the venue was over-filled",
                seed,
                strategy_id,
                acquired_size,
                venue,
                intended_size
            );
            if positions == 1 {
                assert!(
                    acquired_size >= intended_size,
                    "seed {}: mandate {} holds a position but acquired only {} of {} on {:?}",
                    seed,
                    strategy_id,
                    acquired_size,
                    intended_size,
                    venue
                );
            }
        }
    }
    let mut refs: Vec<String> = outcome
        .taker_orders()
        .into_iter()
        .map(|order| order.client_ref.as_str().to_string())
        .collect();
    let before = refs.len();
    refs.sort();
    refs.dedup();
    assert_eq!(
        before,
        refs.len(),
        "seed {}: {} corrective orders reached the venue under only {} distinct client \
         references; a reference was submitted twice",
        seed,
        before,
        refs.len()
    );
}

#[test]
fn seeds_0_to_3_hold_the_safety_properties() {
    for seed in 0..4u64 {
        check_seed(seed);
    }
}

#[test]
fn seeds_4_to_7_hold_the_safety_properties() {
    for seed in 4..8u64 {
        check_seed(seed);
    }
}

#[test]
fn seeds_8_to_11_hold_the_safety_properties() {
    for seed in 8..12u64 {
        check_seed(seed);
    }
}

#[test]
fn seeds_12_to_15_hold_the_safety_properties() {
    for seed in 12..16u64 {
        check_seed(seed);
    }
}

/// The same seed run twice must produce the same venue orders, positions and
/// pending state, field by field.
#[test]
fn the_same_seed_is_repeatable() {
    for seed in 0..6u64 {
        let first = run(interleaving(seed));
        let second = run(interleaving(seed));
        assert_eq!(
            first.venue_orders, second.venue_orders,
            "seed {}: two runs sent different venue orders",
            seed
        );
        assert_eq!(first.positions, second.positions, "seed {}: two runs booked different positions", seed);
        assert_eq!(
            first.pending_orders, second.pending_orders,
            "seed {}: two runs left different pending state",
            seed
        );
    }
}
