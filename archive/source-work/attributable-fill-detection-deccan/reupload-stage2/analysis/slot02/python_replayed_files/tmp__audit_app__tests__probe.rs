// Scratch differential probe: AttributionLedger vs a model written straight from
// /app/spec/venue-semantics.md + /app/spec/reconciliation-contract.md.
mod harness;

use harness::*;
use pairing_engine::engine::AttributionLedger;
use pairing_engine::ports::{BlockFill, FillEvent, OpenOrder};
use pairing_engine::types::ids::{ClientRef, OrderId, Qty};
use pairing_engine::types::snapshot::SubmissionRecord;
use pairing_engine::types::time::Timestamp;
use pairing_engine::ATTRIBUTION_WINDOW;

const OPEN: i64 = 1_000;

// ---------------------------------------------------------------------------
// tiny PRNG
// ---------------------------------------------------------------------------
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

// ---------------------------------------------------------------------------
// scenario
// ---------------------------------------------------------------------------
#[derive(Clone, Debug)]
struct Order {
    client_ref: String,
    order_id: String,
    venue_seq: u64,
    size: Qty,
    window_start: i64,
}

#[derive(Clone, Debug)]
enum Step {
    Event {
        order_idx: usize,
        use_foreign_ref: bool,
        cumulative: Qty,
        at: i64,
    },
    Block {
        block_ref: String,
        covers: Vec<usize>, // indices into orders
        qty: Qty,
        at: i64,
    },
    Open {
        order_idx: usize,
        use_foreign_ref: bool,
        filled: Qty,
    },
}

#[derive(Clone, Debug)]
struct Scenario {
    orders: Vec<Order>,
    steps: Vec<Step>,
}

fn gen(rng: &mut Rng) -> Scenario {
    let bucket_starts = [OPEN, OPEN + 1_000, OPEN + 2_000];
    let id_pool = ["p", "q", "r", "s"];
    let n_orders = 2 + rng.below(4) as usize;
    let mut orders = Vec::new();
    for i in 0..n_orders {
        let bucket = rng.below(3) as usize;
        // ids are recycled across buckets, whose windows are 1000s apart (> 900)
        let id = id_pool[(rng.below(id_pool.len() as u64)) as usize];
        orders.push(Order {
            client_ref: format!("c{i}"),
            order_id: format!("{id}-{bucket}"),
            venue_seq: rng.below(3),
            size: 1 + rng.below(30) as Qty,
            window_start: bucket_starts[bucket],
        });
    }
    // make some records share an order_id label only across disjoint windows:
    // occasionally copy an earlier order's label into a far bucket
    if n_orders >= 3 && rng.below(2) == 0 {
        let src = orders[0].order_id.clone();
        let last = orders.len() - 1;
        orders[last].order_id = src;
    }

    let mut steps = Vec::new();
    let n_steps = 3 + rng.below(8) as usize;
    for _ in 0..n_steps {
        match rng.below(10) {
            0..=4 => steps.push(Step::Event {
                order_idx: rng.below(orders.len() as u64) as usize,
                use_foreign_ref: rng.below(4) == 0,
                cumulative: rng.below(35) as Qty,
                at: OPEN + rng.below(3_200) as i64,
            }),
            5..=8 => {
                let mut covers: Vec<usize> = Vec::new();
                let k = 1 + rng.below(orders.len() as u64) as usize;
                for _ in 0..k {
                    covers.push(rng.below(orders.len() as u64) as usize);
                }
                steps.push(Step::Block {
                    block_ref: format!("b{}", rng.below(3)),
                    covers,
                    qty: rng.below(60) as Qty,
                    at: OPEN + rng.below(3_200) as i64,
                });
            }
            _ => steps.push(Step::Open {
                order_idx: rng.below(orders.len() as u64) as usize,
                use_foreign_ref: rng.below(4) == 0,
                filled: rng.below(35) as Qty,
            }),
        }
    }
    Scenario { orders, steps }
}

// ---------------------------------------------------------------------------
// reference model, straight from the two specs
// ---------------------------------------------------------------------------
#[derive(Clone, Debug, PartialEq)]
struct Model {
    client_ref: ClientRef,
    order_id: Option<OrderId>,
    venue_seq: Option<u64>,
    size: Qty,
    window_start: Timestamp,
    direct: Qty,
    block: Qty,
    finalized: bool,
}

fn secs(v: i64) -> Timestamp {
    Timestamp::from_seconds(v)
}

fn m_in_window(m: &Model, at: Timestamp) -> bool {
    at >= m.window_start && at < m.window_start + ATTRIBUTION_WINDOW
}

fn m_outstanding(m: &Model) -> Qty {
    let r = m.size - (m.direct + m.block);
    if r > 0 {
        r
    } else {
        0
    }
}

fn run_model(scenario: &Scenario) -> Vec<Model> {
    let mut models: Vec<Model> = scenario
        .orders
        .iter()
        .map(|o| Model {
            client_ref: client(&o.client_ref),
            order_id: Some(order(&o.order_id)),
            venue_seq: Some(o.venue_seq),
            size: o.size,
            window_start: secs(o.window_start),
            direct: 0,
            block: 0,
            finalized: false,
        })
        .collect();

    // resolve models carrying a label
    let indices_of = |models: &[Model], id: &OrderId| -> Vec<usize> {
        models
            .iter()
            .enumerate()
            .filter(|(_, m)| m.order_id.as_ref() == Some(id))
            .map(|(i, _)| i)
            .collect()
    };

    let mut absorbed: Vec<String> = Vec::new();
    for step in &scenario.steps {
        match step {
            Step::Event {
                order_idx,
                use_foreign_ref,
                cumulative,
                at,
            } => {
                let src = &scenario.orders[*order_idx];
                let cref = if *use_foreign_ref {
                    client("stranger")
                } else {
                    client(&src.client_ref)
                };
                let oid = order(&src.order_id);
                let when = secs(*at);
                // client_ref is the primary key; else the label, restricted to
                // records whose own window contains the observation.
                let idx = models
                    .iter()
                    .position(|m| m.client_ref == cref)
                    .or_else(|| {
                        indices_of(&models, &oid)
                            .into_iter()
                            .filter(|i| m_in_window(&models[*i], when))
                            .max_by_key(|i| (models[*i].window_start, *i))
                    });
                let Some(idx) = idx else { continue };
                // an outside-the-window observation names an identifier that is
                // no longer this order: it is not attributed, and it does not
                // teach us this order's label either.
                if !m_in_window(&models[idx], when) {
                    continue;
                }
                if models[idx].order_id.is_none() {
                    models[idx].order_id = Some(oid);
                }
                if *cumulative > models[idx].direct {
                    models[idx].direct = *cumulative;
                }
            }
            Step::Open {
                order_idx,
                use_foreign_ref,
                filled,
            } => {
                let src = &scenario.orders[*order_idx];
                let cref = if *use_foreign_ref {
                    client("stranger")
                } else {
                    client(&src.client_ref)
                };
                let oid = order(&src.order_id);
                // open_orders carries no timestamp: always usable, no window test
                let idx = models
                    .iter()
                    .position(|m| m.client_ref == cref)
                    .or_else(|| {
                        indices_of(&models, &oid)
                            .into_iter()
                            .max_by_key(|i| (models[*i].window_start, *i))
                    });
                let Some(idx) = idx else { continue };
                if models[idx].order_id.is_none() {
                    models[idx].order_id = Some(oid);
                }
                if *filled > models[idx].direct {
                    models[idx].direct = *filled;
                }
            }
            Step::Block {
                block_ref,
                covers,
                qty,
                at,
            } => {
                if absorbed.iter().any(|b| b == block_ref) {
                    continue;
                }
                absorbed.push(block_ref.clone());
                let when = secs(*at);

                // eligible set: one entry per tracked record named in covers,
                // inside its own window at `at`, not finalized, outstanding > 0
                let mut elig: Vec<usize> = Vec::new();
                for c in covers {
                    let oid = order(&scenario.orders[*c].order_id);
                    let cands: Vec<usize> = indices_of(&models, &oid)
                        .into_iter()
                        .filter(|i| !models[*i].finalized && m_in_window(&models[*i], when))
                        .collect();
                    let Some(idx) = cands
                        .into_iter()
                        .max_by_key(|i| (models[*i].window_start, *i))
                    else {
                        continue;
                    };
                    if m_outstanding(&models[idx]) <= 0 {
                        continue;
                    }
                    if !elig.contains(&idx) {
                        elig.push(idx);
                    }
                }
                elig.sort_by(|l, r| {
                    models[*l]
                        .window_start
                        .cmp(&models[*r].window_start)
                        .then(models[*l].venue_seq.cmp(&models[*r].venue_seq))
                        .then(
                            models[*l]
                                .order_id
                                .cmp(&models[*r].order_id),
                        )
                });

                let basis: Qty = elig.iter().map(|i| m_outstanding(&models[*i])).sum();
                if basis > 0 {
                    let q = (*qty).min(basis);
                    let mut shares: Vec<Qty> = Vec::new();
                    let mut assigned: Qty = 0;
                    for i in &elig {
                        let w = m_outstanding(&models[*i]);
                        let s = (q * w) / basis;
                        assigned += s;
                        shares.push(s);
                    }
                    let r = q - assigned;
                    for (pos, i) in elig.iter().enumerate() {
                        let extra = if r > 0 && (pos as Qty) < r { 1 } else { 0 };
                        models[*i].block += shares[pos] + extra;
                    }
                }
                // a block finalizes every tracked order it names
                for c in covers {
                    let oid = order(&scenario.orders[*c].order_id);
                    for i in indices_of(&models, &oid) {
                        models[i].finalized = true;
                    }
                }
            }
        }
    }
    models
}

fn run_ledger(scenario: &Scenario) -> Vec<Model> {
    let mut ledger = AttributionLedger::new();
    for o in &scenario.orders {
        ledger.track(
            client(&o.client_ref),
            Some(order(&o.order_id)),
            Some(o.venue_seq),
            o.size,
            secs(o.window_start),
        );
    }
    for step in &scenario.steps {
        match step {
            Step::Event {
                order_idx,
                use_foreign_ref,
                cumulative,
                at,
            } => {
                let src = &scenario.orders[*order_idx];
                let cref = if *use_foreign_ref {
                    "stranger"
                } else {
                    &src.client_ref
                };
                let e = event(&src.order_id, cref, *cumulative, secs(*at));
                let _ = ledger.absorb_batch(batch(vec![e], vec![], "k"));
            }
            Step::Open {
                order_idx,
                use_foreign_ref,
                filled,
            } => {
                let src = &scenario.orders[*order_idx];
                let cref = if *use_foreign_ref {
                    "stranger"
                } else {
                    &src.client_ref
                };
                let o = open_order(&src.order_id, cref, src.venue_seq, src.size, *filled);
                ledger.absorb_open_orders(&[o]);
            }
            Step::Block {
                block_ref,
                covers,
                qty,
                at,
            } => {
                let ids: Vec<&str> = covers
                    .iter()
                    .map(|c| scenario.orders[*c].order_id.as_str())
                    .collect();
                let b = block(block_ref, &ids, *qty, secs(*at));
                let _ = ledger.absorb_batch(batch(vec![], vec![b], "k"));
            }
        }
    }
    let subs: Vec<SubmissionRecord> = ledger.submissions();
    scenario
        .orders
        .iter()
        .map(|o| {
            let rec = subs
                .iter()
                .find(|s| s.client_ref == client(&o.client_ref))
                .expect("every tracked order has a record");
            Model {
                client_ref: rec.client_ref.clone(),
                order_id: rec.order_id.clone(),
                venue_seq: rec.venue_seq,
                size: rec.size,
                window_start: rec.window_start,
                direct: rec.direct_filled,
                block: rec.block_filled,
                finalized: rec.finalized,
            }
        })
        .collect()
}

#[test]
fn differential_fuzz() {
    let mut rng = Rng(0x2545F4914F6CDD1D);
    let mut failures = 0usize;
    let mut first: Option<String> = None;
    for iteration in 0..20_000 {
        let scenario = gen(&mut rng);
        let expected = run_model(&scenario);
        let actual = run_ledger(&scenario);
        if expected != actual {
            failures += 1;
            if first.is_none() {
                let mut report = format!("iteration {iteration}\nscenario: {scenario:#?}\n");
                for (e, a) in expected.iter().zip(actual.iter()) {
                    if e != a {
                        report.push_str(&format!(
                            "  client {:?}\n    expected {:?}\n    actual   {:?}\n",
                            e.client_ref, e, a
                        ));
                    }
                }
                first = Some(report);
            }
        }
    }
    if let Some(report) = first {
        panic!("MISMATCH ({} failing cases). First:\n{}", failures, report);
    }
    assert_eq!(failures, 0);
}

// ---------------------------------------------------------------------------
// determinism: permuting `covers` must not change anything
// ---------------------------------------------------------------------------
fn shuffled(scenario: &Scenario, seed: u64) -> Scenario {
    let mut rng = Rng(seed);
    let mut out = scenario.clone();
    for step in out.steps.iter_mut() {
        if let Step::Block { covers, .. } = step {
            for i in (1..covers.len()).rev() {
                let j = rng.below((i + 1) as u64) as usize;
                covers.swap(i, j);
            }
        }
    }
    out
}

#[test]
fn covers_order_never_changes_the_outcome() {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    for iteration in 0..5_000 {
        let scenario = gen(&mut rng);
        let base = run_ledger(&scenario);
        for seed in [1u64, 7, 99] {
            let alt = run_ledger(&shuffled(&scenario, seed));
            assert_eq!(
                base, alt,
                "iteration {iteration}: covers order decided the outcome\n{scenario:#?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// invariant: a block never allocates more than min(q, basis), and never pushes
// an order past its size
// ---------------------------------------------------------------------------
#[test]
fn block_never_invents_or_overfills() {
    let mut rng = Rng(0xDEADBEEFCAFEBABE);
    for iteration in 0..5_000 {
        let scenario = gen(&mut rng);
        let after = run_ledger(&scenario);
        for (o, m) in scenario.orders.iter().zip(after.iter()) {
            assert!(
                m.direct + m.block <= m.size.max(m.direct),
                "iteration {iteration}: attributed {} exceeds any defensible bound for size {}\n{scenario:#?}",
                m.direct + m.block,
                m.size
            );
            let _ = o;
        }
    }
}

// ---------------------------------------------------------------------------
// targeted probes
// ---------------------------------------------------------------------------

fn tracker(orders: &[(&str, &str, u64, Qty, i64)]) -> AttributionLedger {
    let mut ledger = AttributionLedger::new();
    for (oid, cref, seq, size, start) in orders {
        ledger.track(client(cref), Some(order(oid)), Some(*seq), *size, secs(*start));
    }
    ledger
}

/// order_id is the last tie-break: same window start, same venue_seq (the same
/// venue tick), so the lexicographically smaller label sorts first and takes the
/// remainder unit.
#[test]
fn order_id_breaks_a_full_tie() {
    for ids in [["b-order", "a-order"], ["a-order", "b-order"]] {
        let mut ledger = tracker(&[
            (ids[0], "c1", 5, 10, OPEN),
            (ids[1], "c2", 5, 10, OPEN),
        ]);
        let _ = ledger.absorb_batch(batch(
            vec![],
            vec![block("b1", &ids, 1, secs(OPEN + 10))],
            "k",
        ));
        let a = ledger.attributed_for(&client("c1"));
        let b = ledger.attributed_for(&client("c2"));
        let winner = if a > b { ids[0] } else { ids[1] };
        assert_eq!(
            winner, "a-order",
            "covers {ids:?}: with equal window start and equal venue_seq the smaller \
             order_id sorts first, so 'a-order' takes the remainder unit (c1={a}, c2={b})"
        );
        assert_eq!(a + b, 1, "covers {ids:?}: exactly one base unit was placed");
    }
}

/// A label repeated in covers is one order: the basis counts it once.
#[test]
fn a_repeated_label_counts_once_in_the_basis() {
    let mut ledger = tracker(&[("o1", "c1", 1, 10, OPEN)]);
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["o1", "o1"], 15, secs(OPEN + 10))],
        "k",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        10,
        "a repeated label is one order: basis is 10, so q is min(15,10)=10"
    );
}

/// A block delivered after an order's window closed still closes the order.
#[test]
fn a_late_block_still_finalizes() {
    let mut ledger = tracker(&[("o1", "c1", 1, 10, OPEN)]);
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["o1"], 9, secs(OPEN) + ATTRIBUTION_WINDOW)],
        "k",
    ));
    assert!(ledger.is_finalized(&client("c1")));
    assert_eq!(ledger.outstanding_for(&client("c1")), 0);
}

/// An observation outside the window must not teach the ledger a label.
#[test]
fn an_out_of_window_observation_does_not_bind_a_label() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), None, None, 10, secs(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![event("recycled", "c1", 6, secs(OPEN + 5_000))],
        vec![],
        "k",
    ));
    assert_eq!(ledger.attributed_for(&client("c1")), 0);
    assert_eq!(
        ledger.order_for(&client("c1")),
        None,
        "an observation outside the window names an identifier that is no longer \
         this order, so it must not be recorded as this order's id"
    );
}

/// Two blocks naming the same order: the second sees no outstanding quantity.
#[test]
fn a_second_block_sees_no_basis() {
    let mut ledger = tracker(&[("o1", "c1", 1, 10, OPEN), ("o2", "c2", 2, 10, OPEN)]);
    let _ = ledger.absorb_batch(batch(vec![], vec![block("b1", &["o1", "o2"], 5, secs(OPEN + 10))], "k"));
    let _ = ledger.absorb_batch(batch(vec![], vec![block("b2", &["o1", "o2"], 5, secs(OPEN + 20))], "k"));
    let total = ledger.attributed_for(&client("c1")) + ledger.attributed_for(&client("c2"));
    assert_eq!(total, 5, "the second block has an empty basis");
}
