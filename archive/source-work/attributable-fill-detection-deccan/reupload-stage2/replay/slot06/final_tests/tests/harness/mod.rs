//! In-memory ports for the diagnostics in `tests/diagnostics.rs`.
//!
//! These are yours: `/app/tests` is a scratch area for checking your own work
//! against the documented rules. It is not the graded suite and nothing in here
//! is read when your work is scored. Extend it, rewrite it, or delete it.

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use pairing_engine::ports::{
    BlockFill, Clock, FillBatch, FillEvent, FillEventSource, InventorySource, OpenOrder,
    OrderAccepted, OrderGateway, OrderKind, OrderRequest, PortError, StateStore, StrategySource,
    SubmitError,
};
use pairing_engine::types::ids::{
    BlockRef, ClientRef, InstrumentId, OrderId, Price, Qty, StrategyId, Venue,
};
use pairing_engine::types::snapshot::{Cursor, Snapshot};
use pairing_engine::types::strategy::StrategyCandidate;
use pairing_engine::types::time::Timestamp;

pub fn at(seconds: i64) -> Timestamp {
    Timestamp::from_seconds(seconds)
}

pub fn order(id: &str) -> OrderId {
    OrderId::new(id)
}

pub fn client(id: &str) -> ClientRef {
    ClientRef::new(id)
}

/// A per-order fill event. `cumulative` is what the venue says this order has
/// matched in total so far, not what it matched since the last event.
pub fn event(order_id: &str, client_ref: &str, cumulative: Qty, when: Timestamp) -> FillEvent {
    FillEvent {
        seq: 1,
        order_id: order(order_id),
        client_ref: client(client_ref),
        strategy_id: StrategyId::new("S"),
        instrument_id: InstrumentId::new("I"),
        venue: Venue::North,
        cumulative_filled_qty: cumulative,
        price: 0.5,
        at: when,
    }
}

/// A block match naming several of the engine's orders and reporting one total.
pub fn block(block_ref: &str, covers: &[&str], block_qty: Qty, when: Timestamp) -> BlockFill {
    BlockFill {
        seq: 1,
        block_ref: BlockRef::new(block_ref),
        instrument_id: InstrumentId::new("I"),
        venue: Venue::North,
        covers: covers.iter().map(|id| order(id)).collect(),
        block_qty,
        price: 0.5,
        at: when,
    }
}

pub fn open_order(order_id: &str, client_ref: &str, venue_seq: u64, size: Qty, filled: Qty) -> OpenOrder {
    OpenOrder {
        order_id: order(order_id),
        client_ref: client(client_ref),
        strategy_id: StrategyId::new("S"),
        instrument_id: InstrumentId::new("I"),
        venue: Venue::North,
        kind: OrderKind::RestingLimit,
        venue_seq,
        limit_price: 0.5,
        size,
        filled_qty: filled,
    }
}

pub fn batch(events: Vec<FillEvent>, blocks: Vec<BlockFill>, next_cursor: &str) -> FillBatch {
    FillBatch {
        events,
        blocks,
        next_cursor: Cursor::new(next_cursor),
    }
}

/// A pair candidate both of whose legs are affordable, in range and identical in
/// size, so a diagnostic can vary one thing at a time.
pub fn candidate(strategy_id: &str, depth: Qty, expiry: Timestamp) -> StrategyCandidate {
    candidate_on(
        strategy_id,
        &format!("{strategy_id}-N"),
        &format!("{strategy_id}-S"),
        depth,
        expiry,
    )
}

/// A candidate over named instruments, so two mandates can be made to share one.
pub fn candidate_on(
    strategy_id: &str,
    north: &str,
    south: &str,
    depth: Qty,
    expiry: Timestamp,
) -> StrategyCandidate {
    StrategyCandidate::new(
        StrategyId::new(strategy_id),
        InstrumentId::new(north),
        InstrumentId::new(south),
        0.45,
        0.45,
        depth,
        depth,
        expiry,
    )
}

pub struct FakeClock {
    now: Cell<Timestamp>,
}

impl FakeClock {
    pub fn new(now: Timestamp) -> Self {
        Self { now: Cell::new(now) }
    }

    pub fn set(&self, now: Timestamp) {
        self.now.set(now);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        self.now.get()
    }
}

pub struct FakeInventory {
    pub balance: Cell<Price>,
    pub instruments: RefCell<HashMap<InstrumentId, Qty>>,
}

impl FakeInventory {
    pub fn new(balance: Price) -> Self {
        Self {
            balance: Cell::new(balance),
            instruments: RefCell::new(HashMap::new()),
        }
    }

    /// Seed custody inventory. It aggregates across mandates, across runs and
    /// across orders no longer tracked, so it can never prove that one order
    /// filled; the engine must not read it as a fill.
    pub fn hold(&self, instrument_id: InstrumentId, qty: Qty) {
        self.instruments.borrow_mut().insert(instrument_id, qty);
    }
}

impl InventorySource for FakeInventory {
    fn instrument_inventory(&self, instrument_id: &InstrumentId) -> Result<Qty, PortError> {
        Ok(*self.instruments.borrow().get(instrument_id).unwrap_or(&0))
    }

    fn settlement_balance(&self) -> Result<Price, PortError> {
        Ok(self.balance.get())
    }
}

pub struct FakeStrategies {
    pub candidates: RefCell<Vec<StrategyCandidate>>,
}

impl FakeStrategies {
    pub fn new(candidates: Vec<StrategyCandidate>) -> Self {
        Self {
            candidates: RefCell::new(candidates),
        }
    }
}

impl StrategySource for FakeStrategies {
    fn candidates(&self) -> Result<Vec<StrategyCandidate>, PortError> {
        Ok(self.candidates.borrow().clone())
    }
}

/// A venue that accepts everything unless a diagnostic makes one submission fail.
///
/// `fail_on` names a submission by its one-based position in the sequence of
/// submissions the engine makes. Everything else is accepted with a fresh order
/// id and sequence number. Every request is kept, so a diagnostic can check
/// *what* the engine sent, including which client reference it sent it under.
pub struct FakeGateway {
    failures: RefCell<Vec<(usize, SubmitError)>>,
    submits: Cell<usize>,
    pub submitted: RefCell<Vec<OrderRequest>>,
    pub open: RefCell<Vec<OpenOrder>>,
    pub cancelled: RefCell<Vec<OrderId>>,
    pub best_offer: Cell<Price>,
    /// Made true to have `open_orders` fail. A failure says nothing about the
    /// venue, so the engine may conclude nothing from it.
    pub open_fails: Cell<bool>,
    next_seq: Cell<u64>,
    /// Interleaved timeline of what reached the venue, so a check can assert the
    /// *order* of observable effects and not only that each one happened.
    pub log: RefCell<Vec<String>>,
}

impl FakeGateway {
    pub fn new() -> Self {
        Self {
            failures: RefCell::new(Vec::new()),
            submits: Cell::new(0),
            submitted: RefCell::new(Vec::new()),
            open: RefCell::new(Vec::new()),
            cancelled: RefCell::new(Vec::new()),
            best_offer: Cell::new(0.5),
            open_fails: Cell::new(false),
            next_seq: Cell::new(100),
            log: RefCell::new(Vec::new()),
        }
    }

    /// Position of an entry in the interleaved timeline.
    pub fn logged_at(&self, entry: &str) -> Option<usize> {
        self.log.borrow().iter().position(|seen| seen == entry)
    }

    pub fn fail_on(&self, submission: usize, error: SubmitError) {
        self.failures.borrow_mut().push((submission, error));
    }

    pub fn set_open(&self, open: Vec<OpenOrder>) {
        *self.open.borrow_mut() = open;
    }

    /// Client references of every corrective (immediate-or-cancel) order sent.
    pub fn corrective_refs(&self) -> Vec<String> {
        self.submitted
            .borrow()
            .iter()
            .filter(|request| request.kind == OrderKind::ImmediateOrCancel)
            .map(|request| request.client_ref.to_string())
            .collect()
    }

    /// Sizes of every corrective order sent, in submission order.
    pub fn corrective_sizes(&self) -> Vec<Qty> {
        self.submitted
            .borrow()
            .iter()
            .filter(|request| request.kind == OrderKind::ImmediateOrCancel)
            .map(|request| request.size)
            .collect()
    }
}

impl OrderGateway for FakeGateway {
    fn submit(&self, request: &OrderRequest) -> Result<OrderAccepted, SubmitError> {
        self.submitted.borrow_mut().push(request.clone());
        let position = self.submits.get() + 1;
        self.submits.set(position);
        let failure = {
            let mut failures = self.failures.borrow_mut();
            failures
                .iter()
                .position(|(at_submission, _)| *at_submission == position)
                .map(|index| failures.remove(index).1)
        };
        if let Some(error) = failure {
            self.log
                .borrow_mut()
                .push(format!("submit-failed:{}", request.client_ref));
            return Err(error);
        }
        let seq = self.next_seq.get() + 1;
        self.next_seq.set(seq);
        self.log
            .borrow_mut()
            .push(format!("submit:V-{seq}"));
        Ok(OrderAccepted {
            order_id: OrderId::new(format!("V-{seq}")),
            client_ref: request.client_ref.clone(),
            venue_seq: seq,
        })
    }

    fn cancel(&self, order_id: &OrderId) -> Result<(), PortError> {
        self.cancelled.borrow_mut().push(order_id.clone());
        self.log.borrow_mut().push(format!("cancel:{order_id}"));
        Ok(())
    }

    fn open_orders(&self) -> Result<Vec<OpenOrder>, PortError> {
        if self.open_fails.get() {
            return Err(PortError::Unavailable(String::from("venue busy")));
        }
        Ok(self.open.borrow().clone())
    }

    fn best_offer(&self, _instrument_id: &InstrumentId) -> Result<Option<Price>, PortError> {
        Ok(Some(self.best_offer.get()))
    }
}

/// A feed that hands out one queued batch per poll, then nothing.
pub struct FakeFills {
    pub batches: RefCell<Vec<FillBatch>>,
    pub polled_with: RefCell<Vec<String>>,
}

impl FakeFills {
    pub fn new(batches: Vec<FillBatch>) -> Self {
        Self {
            batches: RefCell::new(batches),
            polled_with: RefCell::new(Vec::new()),
        }
    }
}

impl FillEventSource for FakeFills {
    fn poll(&self, cursor: &Cursor) -> Result<FillBatch, PortError> {
        self.polled_with.borrow_mut().push(cursor.as_str().to_string());
        let mut batches = self.batches.borrow_mut();
        if batches.is_empty() {
            return Ok(batch(Vec::new(), Vec::new(), cursor.as_str()));
        }
        Ok(batches.remove(0))
    }
}

pub struct FakeStore {
    pub saved: RefCell<Option<Snapshot>>,
}

impl FakeStore {
    pub fn empty() -> Self {
        Self {
            saved: RefCell::new(None),
        }
    }

    pub fn holding(snapshot: Snapshot) -> Self {
        Self {
            saved: RefCell::new(Some(snapshot)),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.saved.borrow().clone().expect("engine saved no snapshot")
    }
}

impl StateStore for FakeStore {
    fn load(&self) -> Result<Option<Snapshot>, PortError> {
        Ok(self.saved.borrow().clone())
    }

    fn save(&self, snapshot: &Snapshot) -> Result<(), PortError> {
        *self.saved.borrow_mut() = Some(snapshot.clone());
        Ok(())
    }
}
