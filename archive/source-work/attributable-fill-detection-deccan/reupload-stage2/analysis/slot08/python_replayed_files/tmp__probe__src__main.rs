mod harness;

use harness::*;
use pairing_engine::engine::AttributionLedger;
use pairing_engine::ports::{FillBatch, OrderKind, SubmitError};
use pairing_engine::types::ids::{ClientRef, OrderId, Qty};
use pairing_engine::{Engine, EngineConfig, ATTRIBUTION_WINDOW};
use std::collections::BTreeMap;

const OPEN: i64 = 1_000;

fn north_filled() -> Vec<FillBatch> {
    vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
    ]
}

fn rig(batches: Vec<FillBatch>) -> (FakeClock, FakeStrategies, FakeInventory, FakeGateway, FakeFills, FakeStore) {
    (
        FakeClock::new(at(OPEN)),
        FakeStrategies::new(vec![candidate("m1", 10, at(10_000))]),
        FakeInventory::new(1_000.0),
        FakeGateway::new(),
        FakeFills::new(batches),
        FakeStore::empty(),
    )
}

// ---------------------------------------------------------------------------
// 1. Block division: differential test against a direct transcription of the
//    spec, including the ordering and the remainder rule.
// ---------------------------------------------------------------------------

/// Direct transcription of venue-semantics.md "How a block divides".
fn spec_division(
    orders: &[(String, String, u64, Qty, i64)], // (order_id, client_ref, venue_seq, size, window_start)
    covers: &[&str],
    block_qty: Qty,
    at_ms: i64,
    attributed_before: &BTreeMap<String, (Qty, Qty)>, // client_ref -> (direct, block)
    finalized_before: &BTreeMap<String, bool>,
) -> BTreeMap<String, Qty> {
    let mut out: BTreeMap<String, Qty> = BTreeMap::new();
    // dedup covers, preserving nothing (order is irrelevant)
    let mut seen = std::collections::BTreeSet::new();
    let covers: Vec<&str> = covers.iter().cloned().filter(|c| seen.insert(*c)).cloned().collect();

    let mut eligible: Vec<(String, Qty, (i64, u64, String))> = Vec::new();
    for id in &covers {
        let rec = match orders.iter().find(|(o, _, _, _, _)| o == id) {
            Some(r) => r,
            None => continue,
        };
        let (_oid, cref, vseq, size, wstart) = rec;
        if *finalized_before.get(cref).unwrap_or(&false) {
            continue;
        }
        let (d, b) = attributed_before.get(cref).cloned().unwrap_or((0, 0));
        let outstanding = std::cmp::max(*size - (d + b), 0);
        // half-open window
        if !(at_ms >= *wstart && at_ms < *wstart + ATTRIBUTION_WINDOW.as_millis()) {
            continue;
        }
        if outstanding <= 0 {
            continue;
        }
        eligible.push((cref.clone(), outstanding, (*wstart, *vseq, id.to_string())));
    }
    eligible.sort_by(|a, b| a.2.cmp(&b.2));

    let basis: Qty = eligible.iter().map(|(_, o, _)| *o).sum();
    if basis <= 0 {
        for (oid, cref, _, _, _) in orders {
            if covers.contains(&oid.as_str()) {
                out.insert(cref.clone(), 0);
            }
        }
        return out;
    }
    let q = std::cmp::min(block_qty, basis);
    let mut shares: Vec<Qty> = eligible.iter().map(|(_, o, _)| (q * o) / basis).collect();
    let mut r = q - shares.iter().sum::<Qty>();
    for s in shares.iter_mut() {
        if r <= 0 { break; }
        *s += 1;
        r -= 1;
    }
    for ((cref, _, _), s) in eligible.iter().zip(shares) {
        out.insert(cref.clone(), *s);
    }
    out
}

fn block_division_differential() {
    let mut seed: u64 = 0xABCDEF_1234567;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    let mut failures = 0usize;
    for case in 0..4000 {
        let count = 1 + next() % 5;
        // Sometimes two records share one venue_seq; sometimes window starts differ.
        let orders: Vec<(String, String, u64, Qty, i64)> = (0..count)
            .map(|i| {
                (
                    format!("o{i}"),
                    format!("c{i}"),
                    (next() % 3) as u64,
                    1 + (next() % 12) as Qty,
                    OPEN + (next() % 4) as i64 * 400,
                )
            })
            .collect();

        let mut ledger = AttributionLedger::new();
        let mut attributed_before: BTreeMap<String, (Qty, Qty)> = BTreeMap::new();
        let mut finalized_before: BTreeMap<String, bool> = BTreeMap::new();

        for (oid, cref, vseq, size, wstart) in &orders {
            ledger.track(client(cref), Some(order(oid)), Some(*vseq), *size, at(*wstart));
            attributed_before.insert(cref.clone(), (0, 0));
            finalized_before.insert(cref.clone(), false);
        }

        // Pre-existing direct progress for a random subset (in-window events).
        for (oid, cref, _vseq, size, wstart) in &orders {
            if next() % 3 == 0 {
                let f = (next() % (*size as usize + 1)) as Qty;
                let _ = ledger.absorb_batch(batch(
                    vec![event(oid, cref, f, at(*wstart))],
                    vec![],
                    "pre",
                ));
                attributed_before.insert(cref.clone(), (f, 0));
            }
        }

        // Sometimes finalize one order with an earlier block.
        if count > 1 && next() % 4 == 0 {
            let victim = next() % count;
            let (victim_id, victim_cref, _, _, _) = &orders[victim];
            let _ = ledger.absorb_batch(batch(
                vec![],
                vec![block("pre-block", &[victim_id.as_str()], 1, at(OPEN))],
                "preb",
            ));
            finalized_before.insert(victim_cref.clone(), true);
        }

        let block_at = OPEN + (next() % 5) as i64 * 300;
        let block_qty = (next() % 40) as Qty;
        let covers: Vec<&str> = orders.iter().map(|(o, _, _, _, _)| o.as_str()).collect();
        // shuffle covers
        let mut shuffled: Vec<&str> = covers.clone();
        for i in (1..shuffled.len()).rev() {
            let j = next() % (i + 1);
            shuffled.swap(i, j);
        }

        let _ = ledger.absorb_batch(batch(
            vec![],
            vec![block("bX", &shuffled, block_qty, at(block_at))],
            "k",
        ));

        let expected = spec_division(&orders, &covers, block_qty, block_at, &attributed_before, &finalized_before);
        for (oid, cref, _, size, _) in &orders {
            let got_block_share = ledger.attributed_for(&client(cref))
                - attributed_before.get(cref).map(|(d, _)| *d).unwrap_or(0);
            let want = *expected.get(cref).unwrap_or(&0);
            if got_block_share != want {
                failures += 1;
                if failures < 6 {
                    println!(
                        "DIVISION MISMATCH case {case}: order {oid} block share got {got_block_share} want {want}; \
                         orders={orders:?} block_qty={block_qty} block_at={block_at}"
                    );
                }
            }
            // finalized check
            let is_final = ledger.is_finalized(&client(cref));
            if is_final != (ledger.outstanding_for(&client(cref)) == 0
                && (ledger.attributed_for(&client(cref)) >= *size || *finalized_before.get(cref).unwrap()))
            {
                // informational only
            }
        }
    }
    println!("block division differential: {failures} mismatches over 4000 random cases");
}

// ---------------------------------------------------------------------------
// 2. Scenario probes through run_cycle
// ---------------------------------------------------------------------------

fn engine_of(
    clock: &FakeClock, strategies: &FakeStrategies, inventory: &FakeInventory,
    gateway: &FakeGateway, fills: &FakeFills, store: &FakeStore,
) -> Engine<'_> {
    Engine::new(EngineConfig::default(), strategies, inventory, gateway, fills, store, clock)
}

/// D: an acknowledged IOC that partially fills and is never finalized:
/// does the engine ever cover the rest?
fn probe_partial_ioc() {
    let (clock, strategies, inventory, gateway, fills, store) = rig(north_filled());
    let south = open_order("V-102", "pex-2", 102, 10, 0);
    gateway.set_open(vec![south]);
    let mut engine = engine_of(&clock, &strategies, &inventory, &gateway, &fills, &store);

    engine.run_cycle().unwrap();
    clock.set(at(OPEN + 60));
    engine.run_cycle().unwrap();
    clock.set(at(OPEN + 120));
    engine.run_cycle().unwrap();
    gateway.set_open(vec![]);
    println!("after corrective: sizes={:?}", gateway.corrective_sizes());

    // The IOC (V-103) fills 4 of 10 and the venue cancels the rest. No block.
    fills.batches.borrow_mut().push(batch(
        vec![event("V-103", "pex-3", 4, at(OPEN + 120))],
        vec![],
        "k2",
    ));
    for i in 0..4 {
        clock.set(at(OPEN + 180 + i * 60));
        engine.run_cycle().unwrap();
    }
    println!(
        "partial IOC (4 of 10, venue cancelled rest, no block): corrective sizes={:?} positions={}",
        gateway.corrective_sizes(),
        engine.positions().len()
    );
}

/// E: the resting leg stays visible after cancel (cancel not yet effective).
fn probe_cancel_not_effective() {
    let (clock, strategies, inventory, gateway, fills, store) = rig(north_filled());
    let south = open_order("V-102", "pex-2", 102, 10, 0);
    gateway.set_open(vec![south.clone()]);
    let mut engine = engine_of(&clock, &strategies, &inventory, &gateway, &fills, &store);
    engine.run_cycle().unwrap();
    clock.set(at(OPEN + 60));
    engine.run_cycle().unwrap();
    clock.set(at(OPEN + 120));
    engine.run_cycle().unwrap();
    // venue still lists the leg even though we cancelled it
    gateway.set_open(vec![south]);
    for i in 0..4 {
        clock.set(at(OPEN + 180 + i * 60));
        engine.run_cycle().unwrap();
    }
    println!(
        "cancel-ineffective: corrective sizes={:?} cancels={:?}",
        gateway.corrective_sizes(),
        gateway.cancelled.borrow().len()
    );
}

/// F: closure sequence across a restart.
fn probe_restart_during_closure() {
    let (clock, strategies, inventory, gateway, fills, store) = rig(north_filled());
    let south = open_order("V-102", "pex-2", 102, 10, 0);
    gateway.set_open(vec![south.clone()]);
    {
        let mut engine = engine_of(&clock, &strategies, &inventory, &gateway, &fills, &store);
        engine.run_cycle().unwrap(); // enter
        clock.set(at(OPEN + 60));
        engine.run_cycle().unwrap(); // mark for closure
    }
    let snap = store.snapshot();
    println!(
        "closure restart: closing_orders={:?} corrective so far={:?}",
        snap.closing_orders.iter().map(|o| o.as_str().to_string()).collect::<Vec<_>>(),
        gateway.corrective_sizes()
    );
    {
        let mut engine = engine_of(&clock, &strategies, &inventory, &gateway, &fills, &store);
        clock.set(at(OPEN + 120));
        engine.run_cycle().unwrap();
        println!(
            "  cycle after restart: corrective={:?} cancels={:?}",
            gateway.corrective_sizes(),
            gateway.cancelled.borrow().iter().map(|o| o.as_str().to_string()).collect::<Vec<_>>()
        );
    }
}

/// G: order submitted, acknowledgement lost, then identity recovered from a
/// fill event. Does the retry reuse the same client_ref, and is the IOC counted
/// once?
fn probe_lost_ack_then_identity() {
    let (clock, strategies, inventory, gateway, fills, store) = rig(north_filled());
    let south = open_order("V-102", "pex-2", 102, 10, 0);
    gateway.set_open(vec![south]);
    gateway.fail_on(3, SubmitError::AcknowledgementLost);
    let mut engine = engine_of(&clock, &strategies, &inventory, &gateway, &fills, &store);
    engine.run_cycle().unwrap();
    clock.set(at(OPEN + 60));
    engine.run_cycle().unwrap();
    clock.set(at(OPEN + 120));
    engine.run_cycle().unwrap();
    gateway.set_open(vec![]);
    println!("lost ack: corrective refs={:?}", gateway.corrective_refs());
    // identity arrives on a fill event carrying the client_ref
    fills.batches.borrow_mut().push(batch(
        vec![event("V-999", "pex-3", 10, at(OPEN + 120))],
        vec![],
        "k2",
    ));
    for i in 0..3 {
        clock.set(at(OPEN + 180 + i * 60));
        engine.run_cycle().unwrap();
    }
    println!(
        "  after identity recovered: refs={:?} sizes={:?} positions={}",
        gateway.corrective_refs(),
        gateway.corrective_sizes(),
        engine.positions().len()
    );
}

/// H: rejected neutralization must not latch the mandate shut.
fn probe_rejected_neutralization() {
    let (clock, strategies, inventory, gateway, fills, store) = rig(north_filled());
    let south = open_order("V-102", "pex-2", 102, 10, 0);
    gateway.set_open(vec![south]);
    gateway.fail_on(3, SubmitError::Rejected("nope".into()));
    let mut engine = engine_of(&clock, &strategies, &inventory, &gateway, &fills, &store);
    engine.run_cycle().unwrap();
    clock.set(at(OPEN + 60));
    engine.run_cycle().unwrap();
    clock.set(at(OPEN + 120));
    engine.run_cycle().unwrap();
    gateway.set_open(vec![]);
    println!("rejected once: refs={:?} sizes={:?}", gateway.corrective_refs(), gateway.corrective_sizes());
    for i in 0..3 {
        clock.set(at(OPEN + 180 + i * 60));
        engine.run_cycle().unwrap();
    }
    println!(
        "  after more cycles: refs={:?} sizes={:?} positions={}",
        gateway.corrective_refs(),
        gateway.corrective_sizes(),
        engine.positions().len()
    );
}

/// I: an unconfirmed leg that later reappears in open_orders.
fn probe_unconfirmed_then_resting() {
    let (clock, strategies, inventory, gateway, fills, store) = rig(north_filled());
    gateway.set_open(vec![]); // leg never visible
    let mut engine = engine_of(&clock, &strategies, &inventory, &gateway, &fills, &store);
    engine.run_cycle().unwrap();
    clock.set(at(OPEN + 60));
    engine.run_cycle().unwrap();
    println!("unconfirmed: corrective={:?}", gateway.corrective_sizes());
    gateway.set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
    for i in 0..4 {
        clock.set(at(OPEN + 120 + i * 60));
        engine.run_cycle().unwrap();
    }
    println!(
        "  after leg reappears: corrective={:?} cancels={:?} positions={}",
        gateway.corrective_sizes(),
        gateway.cancelled.borrow().len(),
        engine.positions().len()
    );
}

/// J: open_orders entry whose client_ref is unknown but whose order_id matches a
/// tracked order (id recycling / foreign order reusing the id).
fn probe_open_order_id_collision() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    let _ = ledger.absorb_batch(batch(vec![event("o1", "c1", 4, at(OPEN))], vec![], "k"));
    // A *different* live order now carries the recycled id o1, under a client_ref
    // this engine does not know.
    ledger.absorb_open_orders(&[open_order("o1", "stranger", 7, 10, 9)]);
    println!(
        "open_order id collision (recycled id, foreign client_ref): c1 attributed = {}",
        ledger.attributed_for(&client("c1"))
    );
}

/// K: two records end up holding the same order_id; which one does an
/// observation naming only the order_id resolve to?
fn probe_duplicate_order_id() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), None, None, 10, at(OPEN));
    // An observation naming o1 under c2 (the venue says o1 carries c2):
    ledger.absorb_open_orders(&[open_order("o1", "c2", 5, 10, 6)]);
    println!(
        "duplicate id: c1={} c2={} (venue said o1 has client_ref c2)",
        ledger.attributed_for(&client("c1")),
        ledger.attributed_for(&client("c2"))
    );
}

/// L: fill event delivered AFTER a block that covered the order, same batch.
fn probe_block_then_event() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    let b = FillBatch {
        events: vec![event("o1", "c1", 4, at(OPEN + 10))],
        blocks: vec![block("b1", &["o1"], 6, at(OPEN + 20))],
        next_cursor: harness::Cursor::new("k"),
    };
    let _ = ledger.absorb_batch(b);
    println!("block+event same batch (event before block in time): o1 = {}", ledger.attributed_for(&client("c1")));

    let mut ledger2 = AttributionLedger::new();
    ledger2.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    let _ = ledger2.absorb_batch(batch(vec![], vec![block("b1", &["o1"], 6, at(OPEN + 20))], "k1"));
    let _ = ledger2.absorb_batch(batch(vec![event("o1", "c1", 4, at(OPEN + 10))], vec![], "k2"));
    println!("block first, event later:                              o1 = {}", ledger2.attributed_for(&client("c1")));
}

/// M: does the engine ever send two IOCs in one cycle, or an IOC before a cancel?
fn probe_ordering_of_cancellations() {
    let (clock, strategies, inventory, gateway, fills, store) = rig(north_filled());
    let south = open_order("V-102", "pex-2", 102, 10, 0);
    gateway.set_open(vec![south.clone()]);
    let mut engine = engine_of(&clock, &strategies, &inventory, &gateway, &fills, &store);
    for i in 0..6 {
        clock.set(at(OPEN + i * 60));
        gateway.set_open(vec![south.clone()]);
        engine.run_cycle().unwrap();
        let subs = gateway.submitted.borrow();
        let last: Vec<String> = subs
            .iter()
            .map(|r| format!("{:?}:{}", r.kind, r.client_ref))
            .collect();
        let cancels = gateway.cancelled.borrow().len();
        println!("  cycle {i}: cancels={cancels} submissions={last:?}");
    }
}

/// N: full restart mid-flight with an unacknowledged leg attempt.
fn probe_leg_attempt_restart() {
    let (clock, strategies, inventory, gateway, fills, store) = rig(vec![batch(vec![], vec![], "k0")]);
    gateway.fail_on(2, SubmitError::AcknowledgementLost); // south leg
    {
        let mut engine = engine_of(&clock, &strategies, &inventory, &gateway, &fills, &store);
        engine.run_cycle().unwrap();
    }
    let snap = store.snapshot();
    println!(
        "leg attempt restart: leg_attempts={} pending={} client_seq={}",
        snap.leg_attempts.len(),
        snap.pending_orders.len(),
        snap.client_seq
    );
    {
        let mut engine = engine_of(&clock, &strategies, &inventory, &gateway, &fills, &store);
        clock.set(at(OPEN + 60));
        engine.run_cycle().unwrap();
        let subs = gateway.submitted.borrow();
        let refs: Vec<String> = subs.iter().map(|r| r.client_ref.to_string()).collect();
        println!("  after restart refs={refs:?} pending={}", engine.pending_orders().len());
    }
}

fn main() {
    println!("== block division differential ==");
    block_division_differential();
    println!("\n== D partial IOC ==");
    probe_partial_ioc();
    println!("\n== E cancel not effective ==");
    probe_cancel_not_effective();
    println!("\n== F closure across restart ==");
    probe_restart_during_closure();
    println!("\n== G lost ack then identity ==");
    probe_lost_ack_then_identity();
    println!("\n== H rejected neutralization ==");
    probe_rejected_neutralization();
    println!("\n== I unconfirmed then resting ==");
    probe_unconfirmed_then_resting();
    println!("\n== J open_order id collision ==");
    probe_open_order_id_collision();
    println!("\n== K duplicate order id ==");
    probe_duplicate_order_id();
    println!("\n== L block then event ==");
    probe_block_then_event();
    println!("\n== M ordering of cancellations ==");
    probe_ordering_of_cancellations();
    println!("\n== N leg attempt restart ==");
    probe_leg_attempt_restart();
}
