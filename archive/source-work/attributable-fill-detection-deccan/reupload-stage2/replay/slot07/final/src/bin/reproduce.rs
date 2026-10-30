//! Offline self-checking reproducer for the fill-attribution defects.
//!
//! This is a diagnostic aid, not the grader. It drives the engine through the
//! public surface with in-memory fakes and prints one PASS/FAIL line per check,
//! so a change can be evaluated without guessing.
//!
//! Run with:
//! ```sh
//! cargo run --offline --bin reproduce
//! ```
//!
//! Exits 0 when every check passes and 1 otherwise, so it can be used in a loop.
//!
//! Scope. These checks cover the coarse attribution failures: inventory that
//! predates an order, one instrument shared by two mandates, a stale feed, a
//! genuine one-legged fill and a restart. They are necessary, not sufficient.
//! The graded suite is broader and checks the exact rules in
//! `/app/spec/venue-semantics.md` — attribution window boundaries, integer
//! division of a block, the remainder tie-break, block idempotency — which this
//! file deliberately does not encode. `tests/diagnostics.rs` does check those,
//! one rule at a time. Passing everything here says the coarse model is right;
//! it says nothing about the exact ones.
//!
//! No network and no venue account. The venues, the custody account and the
//! event feed are fakes driven by a script. Extend it freely: it is not graded,
//! and the verifier discards `src/bin/` before grading.

use pairing_engine::{
    Clock, Cursor, Duration, Engine, EngineConfig, FillBatch, FillEvent, FillEventSource,
    InstrumentId, InventorySource, OpenOrder, OrderAccepted, OrderGateway, OrderId, OrderKind,
    OrderRequest, PortError, Price, Qty, Snapshot, StateStore, StrategyCandidate, StrategyId,
    StrategySource, Timestamp, Venue,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Check reporting
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Report {
    passed: usize,
    failed: usize,
}

impl Report {
    /// Record one check. `detail` is printed either way, because a passing
    /// check whose reason is invisible is not much use when the next one fails.
    fn check(&mut self, name: &str, ok: bool, detail: impl AsRef<str>) {
        if ok {
            self.passed += 1;
            println!("  PASS  {name}");
        } else {
            self.failed += 1;
            println!("  FAIL  {name}");
        }
        let detail = detail.as_ref();
        if !detail.is_empty() {
            println!("          {detail}");
        }
    }
}

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

struct FakeClock {
    now: Cell<Timestamp>,
}

impl FakeClock {
    fn new(t: Timestamp) -> Self {
        Self { now: Cell::new(t) }
    }

    fn advance(&self, delta: Duration) {
        self.now.set(self.now.get() + delta);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        self.now.get()
    }
}

struct FakeInventory {
    instruments: RefCell<HashMap<InstrumentId, Qty>>,
    cash: RefCell<Price>,
}

impl FakeInventory {
    fn new() -> Self {
        Self {
            instruments: RefCell::new(HashMap::new()),
            cash: RefCell::new(1000.0),
        }
    }

    fn set_instrument(&self, instrument_id: InstrumentId, qty: Qty) {
        self.instruments.borrow_mut().insert(instrument_id, qty);
    }
}

impl InventorySource for FakeInventory {
    fn instrument_inventory(&self, instrument_id: &InstrumentId) -> Result<Qty, PortError> {
        Ok(*self.instruments.borrow().get(instrument_id).unwrap_or(&0))
    }

    fn settlement_balance(&self) -> Result<Price, PortError> {
        Ok(*self.cash.borrow())
    }
}

struct FakeGateway {
    next_order_id: RefCell<u64>,
    submitted: RefCell<Vec<OrderRequest>>,
    open_orders: RefCell<Vec<OpenOrder>>,
}

impl FakeGateway {
    fn new() -> Self {
        Self {
            next_order_id: RefCell::new(1),
            submitted: RefCell::new(Vec::new()),
            open_orders: RefCell::new(Vec::new()),
        }
    }

    /// Seed the open-order list the engine reads back. This is the hook for
    /// order-visibility scenarios: an order absent from this list is not
    /// necessarily gone, and the engine has to cope with that.
    fn set_open_orders(&self, orders: Vec<OpenOrder>) {
        *self.open_orders.borrow_mut() = orders;
    }

    /// Every request the engine has submitted so far, in order.
    fn submitted(&self) -> Vec<OrderRequest> {
        self.submitted.borrow().clone()
    }

    fn taker_orders(&self) -> Vec<OrderRequest> {
        self.submitted()
            .into_iter()
            .filter(|r| r.kind == OrderKind::ImmediateOrCancel)
            .collect()
    }

    /// The order id the venue handed back for a given client reference.
    fn order_id_for(&self, client_ref: &str) -> Option<OrderId> {
        self.submitted()
            .iter()
            .position(|r| r.client_ref.as_str() == client_ref)
            .map(|i| OrderId::new(format!("ord{}", i + 1)))
    }
}

impl OrderGateway for FakeGateway {
    fn submit(&self, request: &OrderRequest) -> Result<OrderAccepted, pairing_engine::SubmitError> {
        let order_id = OrderId::new(format!("ord{}", *self.next_order_id.borrow()));
        *self.next_order_id.borrow_mut() += 1;
        self.submitted.borrow_mut().push(request.clone());
        let venue_seq = *self.next_order_id.borrow();
        Ok(OrderAccepted {
            order_id,
            client_ref: request.client_ref.clone(),
            venue_seq,
        })
    }

    fn cancel(&self, _order_id: &OrderId) -> Result<(), PortError> {
        Ok(())
    }

    fn open_orders(&self) -> Result<Vec<OpenOrder>, PortError> {
        Ok(self.open_orders.borrow().clone())
    }

    fn best_offer(&self, _instrument_id: &InstrumentId) -> Result<Option<Price>, PortError> {
        Ok(Some(0.51))
    }
}

struct FakeStrategies {
    candidates: RefCell<Vec<StrategyCandidate>>,
}

impl FakeStrategies {
    fn new() -> Self {
        Self {
            candidates: RefCell::new(Vec::new()),
        }
    }

    fn set_candidates(&self, candidates: Vec<StrategyCandidate>) {
        *self.candidates.borrow_mut() = candidates;
    }
}

impl StrategySource for FakeStrategies {
    fn candidates(&self) -> Result<Vec<StrategyCandidate>, PortError> {
        Ok(self.candidates.borrow().clone())
    }
}

/// Fill feed. Delivery is at-least-once, so `poll` replays everything it holds
/// on every call, exactly as the real feed is permitted to.
struct FakeFills {
    events: RefCell<Vec<FillEvent>>,
}

impl FakeFills {
    fn new() -> Self {
        Self {
            events: RefCell::new(Vec::new()),
        }
    }

    fn push(&self, event: FillEvent) {
        self.events.borrow_mut().push(event);
    }
}

impl FillEventSource for FakeFills {
    fn poll(&self, _cursor: &Cursor) -> Result<FillBatch, PortError> {
        Ok(FillBatch {
            events: self.events.borrow().clone(),
            blocks: Vec::new(),
            next_cursor: Cursor::beginning(),
        })
    }
}

struct FakeStore {
    snapshot: RefCell<Option<Snapshot>>,
}

impl FakeStore {
    fn new() -> Self {
        Self {
            snapshot: RefCell::new(None),
        }
    }
}

impl StateStore for FakeStore {
    fn load(&self) -> Result<Option<Snapshot>, PortError> {
        Ok(self.snapshot.borrow().clone())
    }

    fn save(&self, snapshot: &Snapshot) -> Result<(), PortError> {
        *self.snapshot.borrow_mut() = Some(snapshot.clone());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Shared fixtures
// ---------------------------------------------------------------------------

fn config() -> EngineConfig {
    EngineConfig {
        max_pair_cost: 0.98,
        min_leg_size: 5,
        max_leg_size: 50,
        neutralization_slippage: 0.02,
        client_ref_prefix: String::from("test"),
    }
}

fn candidate(id: &str, north: &str, south: &str, size: Qty) -> StrategyCandidate {
    StrategyCandidate::new(
        StrategyId::from(id),
        InstrumentId::from(north),
        InstrumentId::from(south),
        0.45,
        0.52,
        size,
        size,
        Timestamp::from_seconds(9_000),
    )
}

fn fill_event(seq: u64, order: &OrderId, client_ref: &str, qty: Qty, at: Timestamp) -> FillEvent {
    FillEvent {
        seq,
        order_id: order.clone(),
        client_ref: client_ref.into(),
        strategy_id: StrategyId::from("unused"),
        instrument_id: InstrumentId::from("unused"),
        venue: Venue::North,
        cumulative_filled_qty: qty,
        price: 0.45,
        at,
    }
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

/// Custody already holds exactly one leg's worth before the mandate starts.
/// Nothing was acquired for this order, so no pairing may be recorded.
fn scenario_pre_existing_inventory(report: &mut Report) {
    println!("\n[1] Pre-existing custody inventory is not a fill");
    println!("    Custody holds 30 units of the North instrument from an earlier session.");
    println!("    The engine submits a new 30-unit North leg. Nothing fills.");

    let clock = FakeClock::new(Timestamp::from_seconds(1_000));
    let inventory = FakeInventory::new();
    let gateway = FakeGateway::new();
    let strategies = FakeStrategies::new();
    let fills = FakeFills::new();
    let store = FakeStore::new();

    inventory.set_instrument(InstrumentId::from("inst_north_s1"), 30);
    strategies.set_candidates(vec![candidate("strategy1", "inst_north_s1", "inst_south_s1", 30)]);

    let mut engine = Engine::new(
        config(),
        &strategies,
        &inventory,
        &gateway,
        &fills,
        &store,
        &clock,
    );

    engine.run_cycle().unwrap();
    let pending_after_entry = engine.pending_orders().len();
    report.check(
        "both legs are submitted and tracked",
        pending_after_entry == 2,
        format!("expected 2 tracked orders, saw {pending_after_entry}"),
    );

    clock.advance(Duration::from_seconds(10));
    engine.run_cycle().unwrap();

    let positions = engine.positions();
    report.check(
        "no pairing is recorded from pre-existing inventory",
        positions.is_empty(),
        format!(
            "expected 0 positions, saw {}. The 30 units predate the order and prove nothing about it.",
            positions.len()
        ),
    );

    let takers = gateway.taker_orders().len();
    report.check(
        "no corrective order is sent when neither leg filled",
        takers == 0,
        format!("expected 0 taker orders, saw {takers}"),
    );

    let still_tracked = engine.pending_orders().len();
    report.check(
        "both live legs remain tracked",
        still_tracked == 2,
        format!("expected 2 tracked orders, saw {still_tracked}"),
    );
}

/// Two mandates on one instrument identifier. A fill for the first must not
/// complete a leg of the second.
fn scenario_shared_instrument(report: &mut Report) {
    println!("\n[2] A shared instrument identifier does not cross-attribute");
    println!("    Mandates A and B both reference inst_north_shared.");
    println!("    Only A's North leg fills, and only A's 30 units arrive in custody.");

    let clock = FakeClock::new(Timestamp::from_seconds(1_000));
    let inventory = FakeInventory::new();
    let gateway = FakeGateway::new();
    let strategies = FakeStrategies::new();
    let fills = FakeFills::new();
    let store = FakeStore::new();

    strategies.set_candidates(vec![
        candidate("strategyA", "inst_north_shared", "inst_south_a", 30),
        candidate("strategyB", "inst_north_shared", "inst_south_b", 30),
    ]);

    let mut engine = Engine::new(
        config(),
        &strategies,
        &inventory,
        &gateway,
        &fills,
        &store,
        &clock,
    );

    engine.run_cycle().unwrap();
    let tracked = engine.pending_orders().len();
    report.check(
        "all four legs are submitted and tracked",
        tracked == 4,
        format!("expected 4 tracked orders, saw {tracked}"),
    );

    // Mandate A's North leg fills for its full size. Custody rises to 30 on the
    // shared identifier, which is also mandate B's North instrument.
    let at = clock.now();
    if let Some(order) = gateway.order_id_for("test-1") {
        fills.push(fill_event(1, &order, "test-1", 30, at));
    }
    inventory.set_instrument(InstrumentId::from("inst_north_shared"), 30);

    clock.advance(Duration::from_seconds(10));
    engine.run_cycle().unwrap();

    let positions = engine.positions();
    let b_completed = positions.iter().any(|p| p.strategy_id.as_str() == "strategyB");
    report.check(
        "mandate B records no pairing from mandate A's fill",
        !b_completed,
        "expected strategyB to record nothing: those 30 units belong to strategyA",
    );

    let a_takers = gateway
        .taker_orders()
        .iter()
        .filter(|r| r.strategy_id.as_str() == "strategyA")
        .count();
    report.check(
        "mandate A's unfilled South leg draws at most one corrective order",
        a_takers <= 1,
        format!("expected 0 or 1 corrective orders for strategyA, saw {a_takers}"),
    );
}

/// The feed repeats the same event for many cycles. Redelivery is normal and
/// must be absorbed without duplicating anything.
fn scenario_stale_and_repeated(report: &mut Report) {
    println!("\n[3] Repeated and stale observations do not compound");
    println!("    One genuine fill for the North leg, then eight cycles of the same");
    println!("    event redelivered and a frozen custody figure.");

    let clock = FakeClock::new(Timestamp::from_seconds(1_000));
    let inventory = FakeInventory::new();
    let gateway = FakeGateway::new();
    let strategies = FakeStrategies::new();
    let fills = FakeFills::new();
    let store = FakeStore::new();

    strategies.set_candidates(vec![candidate("strategy1", "inst_north_s1", "inst_south_s1", 30)]);

    let mut engine = Engine::new(
        config(),
        &strategies,
        &inventory,
        &gateway,
        &fills,
        &store,
        &clock,
    );

    engine.run_cycle().unwrap();

    let at = clock.now();
    if let Some(order) = gateway.order_id_for("test-1") {
        fills.push(fill_event(1, &order, "test-1", 30, at));
    }
    inventory.set_instrument(InstrumentId::from("inst_north_s1"), 30);

    for _ in 0..8 {
        clock.advance(Duration::from_seconds(10));
        engine.run_cycle().unwrap();
    }

    let positions = engine.positions();
    report.check(
        "eight cycles of redelivery record at most one pairing",
        positions.len() <= 1,
        format!("expected 0 or 1 positions, saw {}", positions.len()),
    );

    let takers = gateway.taker_orders().len();
    report.check(
        "redelivery does not multiply corrective orders",
        takers <= 1,
        format!(
            "expected at most 1 corrective order across 8 cycles, saw {takers}. \
             The same observation arriving again is not new information."
        ),
    );
}

/// One leg genuinely fills and the other does not. That is a real exposure and
/// it must draw exactly one corrective order, sized to the gap.
fn scenario_one_legged_fill(report: &mut Report) {
    println!("\n[4] A genuine one-legged fill is neutralized once");
    println!("    The North leg fills for its full 30 units. The South leg does not.");

    let clock = FakeClock::new(Timestamp::from_seconds(1_000));
    let inventory = FakeInventory::new();
    let gateway = FakeGateway::new();
    let strategies = FakeStrategies::new();
    let fills = FakeFills::new();
    let store = FakeStore::new();

    strategies.set_candidates(vec![candidate("strategy1", "inst_north_s1", "inst_south_s1", 30)]);

    let mut engine = Engine::new(
        config(),
        &strategies,
        &inventory,
        &gateway,
        &fills,
        &store,
        &clock,
    );

    engine.run_cycle().unwrap();

    let north = gateway.order_id_for("test-1");
    let south = gateway.order_id_for("test-2");
    let at = clock.now();
    if let Some(order) = &north {
        fills.push(fill_event(1, order, "test-1", 30, at));
    }

    // The South leg is still resting and visible; the North leg has left the
    // book because it filled.
    if let (Some(south_id), Some(request)) = (south, gateway.submitted().get(1).cloned()) {
        gateway.set_open_orders(vec![OpenOrder {
            order_id: south_id,
            client_ref: request.client_ref.clone(),
            strategy_id: request.strategy_id.clone(),
            instrument_id: request.instrument_id.clone(),
            venue: Venue::South,
            kind: OrderKind::RestingLimit,
            venue_seq: 3,
            limit_price: request.limit_price,
            size: request.size,
            filled_qty: 0,
        }]);
    }

    for _ in 0..4 {
        clock.advance(Duration::from_seconds(10));
        engine.run_cycle().unwrap();
    }

    let takers = gateway.taker_orders();
    report.check(
        "the open exposure draws exactly one corrective order",
        takers.len() == 1,
        format!(
            "expected exactly 1 corrective order, saw {}. Progress matters as much as safety: \
             a genuine one-legged fill must still be neutralized.",
            takers.len()
        ),
    );

    let taker_size = takers.first().map(|taker| taker.size);
    report.check(
        "the corrective order is sized to the unresolved quantity",
        taker_size == Some(30),
        match taker_size {
            Some(size) => format!("expected size 30, saw {size}"),
            None => String::from("expected size 30, saw no corrective order at all"),
        },
    );
}

/// State is restored from the store, and custody still holds what it held.
/// Neither may be reinterpreted as new activity.
fn scenario_restart(report: &mut Report) {
    println!("\n[5] A restart does not reinterpret old inventory");
    println!("    Two legs are tracked, custody holds 30 pre-existing units, the");
    println!("    process restarts and reloads from the store.");

    let clock = FakeClock::new(Timestamp::from_seconds(1_000));
    let inventory = FakeInventory::new();
    let gateway = FakeGateway::new();
    let strategies = FakeStrategies::new();
    let fills = FakeFills::new();
    let store = FakeStore::new();

    inventory.set_instrument(InstrumentId::from("inst_north_s1"), 30);
    strategies.set_candidates(vec![candidate("strategy1", "inst_north_s1", "inst_south_s1", 30)]);

    {
        let mut engine = Engine::new(
            config(),
            &strategies,
            &inventory,
            &gateway,
            &fills,
            &store,
            &clock,
        );
        engine.run_cycle().unwrap();
        clock.advance(Duration::from_seconds(10));
        engine.run_cycle().unwrap();
    }

    // Restart: a fresh Engine over the same store, venue and custody account.
    let mut engine = Engine::new(
        config(),
        &strategies,
        &inventory,
        &gateway,
        &fills,
        &store,
        &clock,
    );

    clock.advance(Duration::from_seconds(10));
    engine.run_cycle().unwrap();

    let positions = engine.positions();
    report.check(
        "the restart records no pairing from pre-existing inventory",
        positions.is_empty(),
        format!(
            "expected 0 positions after restart, saw {}. A restart clears process memory \
             but not the custody account.",
            positions.len()
        ),
    );

    let takers = gateway.taker_orders().len();
    report.check(
        "the restart sends no corrective order",
        takers == 0,
        format!("expected 0 corrective orders, saw {takers}"),
    );

    let tracked = engine.pending_orders().len();
    report.check(
        "unresolved orders survive the restart",
        tracked == 2,
        format!(
            "expected 2 tracked orders after restart, saw {tracked}. Live orders must remain \
             recoverable."
        ),
    );
}

fn main() {
    println!("Offline reproducer for /app — fill attribution diagnostics");
    println!("Diagnostic only. The graded suite is broader; see instruction.md.");

    let mut report = Report::default();
    scenario_pre_existing_inventory(&mut report);
    scenario_shared_instrument(&mut report);
    scenario_stale_and_repeated(&mut report);
    scenario_one_legged_fill(&mut report);
    scenario_restart(&mut report);

    let total = report.passed + report.failed;
    println!("\n=== {} passed, {} failed, {total} checks ===", report.passed, report.failed);

    if report.failed > 0 {
        println!("\nThese are coarse, whole-cycle checks. A failure above means the engine's");
        println!("reading of the venue disagrees with /app/spec/venue-semantics.md somewhere");
        println!("that a five-scenario walkthrough can see.");
        println!("\nThe finer rules - window boundaries, the exact division of a block, the");
        println!("remainder tie-break, redelivery - are not visible here. Check those with");
        println!("`cargo test --offline --test diagnostics`.");
        println!("\nSee /app/spec/source-map.md for where each concern lives.");
        std::process::exit(1);
    }

    println!("\nThese coarse checks pass. They do not cover the exact rules in");
    println!("/app/spec/venue-semantics.md: attribution window boundaries, integer");
    println!("division of a block fill, the remainder tie-break, or block idempotency.");
    println!("For those, run `cargo test --offline --test diagnostics`.");
    println!("Both are diagnostics, not the grader.");
}
