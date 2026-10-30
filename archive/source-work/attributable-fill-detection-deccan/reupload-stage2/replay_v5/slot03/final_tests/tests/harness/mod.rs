#![allow(dead_code)]
#![allow(unused_imports)]
//! Deterministic fake venue and scenario harness for the diagnostics under
//! `/app/tests`.
//!
//! The diagnostics drive the engine only through its public constructor and its
//! six injected ports. Everything the engine can observe is faked here, so a
//! scenario is a description of what the venue does rather than a sequence of
//! calls. The fakes follow `/app/spec/venue-semantics.md`; this file is yours to
//! extend when a scenario needs something the venue can do and the fake cannot.
//!
//! # Model
//!
//! A `Scenario` is a list of per-cycle `Action`s. `run` builds the fakes, steps
//! the engine one cycle per entry, and returns an `Outcome` recording every
//! interaction: orders submitted, cancels, persisted snapshots, positions.
//!
//! Time is a virtual `Clock` advanced explicitly, and randomness comes from a
//! seeded `Prng`, so a scenario replays identically.
//!
//! # Deliberate properties
//!
//! - **Orders are addressed by strategy and venue.** Helpers such as `fill_resting`
//!   and `close` resolve the identifier at run time from what the engine actually
//!   submitted, so no test can hardcode a fixture id and no correct
//!   implementation is forced into a particular naming scheme.
//! - **The fill feed is at-least-once.** It replays, repeats cursors, and can be
//!   withheld entirely. Events carry a *cumulative* filled quantity, so a correct
//!   consumer records a maximum rather than a sum.
//! - **Quantities are whole base units.** `Qty` is an integer and nothing here
//!   rounds, so a scenario can pin an exact single-unit outcome.
//! - **Order identifiers are opaque.** The venue labels orders from a fixed
//!   scrambled alphabet, so lexicographic order deliberately does not match
//!   submission order. A tie-break that reaches for the identifier first, or that
//!   lets a map's iteration order decide, gets a different answer.
//! - **The venue ticks.** Every cycle is one venue tick, and all orders accepted
//!   in a tick share its sequence number.
//! - **The venue deduplicates on client reference.** Resubmitting the same
//!   reference returns the original order, which is what makes recovery from a
//!   lost acknowledgement observable.
//! - **Ports can fail.** `PortError` and `SubmitError::AcknowledgementLost` are
//!   injectable, including an inventory source frozen for the whole run.
//! - **State survives restarts.** The `StateStore` fake keeps the persisted
//!   snapshot across engine instances so restart scenarios are honest.
//!
//! The harness names no internal type of the engine.

use std::cell::RefCell;
use std::collections::BTreeMap;

use pairing_engine::engine::{Engine, EngineConfig};
pub use pairing_engine::ports::{
    BlockFill, Clock, FillBatch, FillEvent, FillEventSource, InventorySource, OpenOrder,
    OrderAccepted, OrderGateway, OrderKind, OrderRequest, PortError, StateStore, StrategySource,
    SubmitError,
};
pub use pairing_engine::types::{
    BlockRef, ClientRef, Cursor, InstrumentId, OrderId, PendingOrder, Position, Price, Qty,
    Snapshot, StrategyCandidate, StrategyId, Timestamp, Venue, ATTRIBUTION_WINDOW, QTY_UNIT,
};

/// Price comparison tolerance. Prices are real numbers; quantities are not, and
/// nothing in this harness compares a quantity approximately.
pub const PRICE_TOLERANCE: f64 = 1e-6;

/// The venue's opaque order label alphabet.
///
/// Deliberately not in lexicographic order: the nth order accepted is labelled
/// `ALPHABET[n % 8]-<n / 8>`, so sorting labels as strings scrambles submission
/// order. Real venue identifiers carry no ordering information either.
const ORDER_LABELS: [&str; 8] = ["m", "c", "t", "s", "a", "r", "f", "p"];

/// The label the venue gives the `index`th order it accepts.
fn venue_label(index: u64) -> String {
    format!("{}-{}", ORDER_LABELS[(index % 8) as usize], index / 8)
}

/// Compare two prices within `PRICE_TOLERANCE`. Used only for prices; a test
/// that means to pin a quantity compares it exactly.
pub fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < PRICE_TOLERANCE
}

/// Build a strategy identifier from its public string form.
pub fn strategy(id: &str) -> StrategyId {
    StrategyId::new(id)
}

/// Build an instrument identifier from its public string form.
pub fn instrument(id: &str) -> InstrumentId {
    InstrumentId::new(id)
}

/// What the fake venue did with one submission, as the harness observed it.
///
/// `Deduplicated` and `AcknowledgementLost` are the two outcomes a naive
/// gateway client conflates with `Accepted`: the first means the venue matched
/// an earlier client reference and returned the *original* order rather than a
/// new one, the second means the order exists at the venue but the engine never
/// learned its identifier. Tests distinguish them to show that recovery after a
/// lost acknowledgement reattaches to the existing order instead of duplicating
/// it.
#[derive(Debug, Clone, PartialEq)]
pub enum SubmitOutcome {
    Accepted(OrderId),
    Deduplicated(OrderId),
    AcknowledgementLost(OrderId),
    Rejected,
}

/// One submission as the engine issued it, recorded whether or not the venue
/// created a new order. `Outcome::attempts` is therefore a record of intent:
/// duplicate submissions appear here even when the venue collapses them into a
/// single `VenueOrder`.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmitAttempt {
    pub client_ref: ClientRef,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    pub kind: OrderKind,
    pub limit_price: Price,
    pub size: Qty,
    pub outcome: SubmitOutcome,
}

/// One order as it exists *at the venue*, which is the only authority on order
/// state in these tests.
///
/// `filled_qty` is cumulative, matching the at-least-once feed. `venue_seq` is
/// the tick that accepted the order and is shared by every order accepted in
/// the same tick, so it identifies a batch and not an order. `live` and
/// `cancelled` are the venue's view, not the engine's belief about it.
#[derive(Debug, Clone, PartialEq)]
pub struct VenueOrder {
    pub order_id: OrderId,
    pub client_ref: ClientRef,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    pub kind: OrderKind,
    pub limit_price: Price,
    pub size: Qty,
    pub filled_qty: Qty,
    pub venue_seq: u64,
    pub live: bool,
    pub cancelled: bool,
}

/// Addresses an order the engine submitted, by role rather than identifier.
///
/// The harness resolves this to a concrete order at run time from what the
/// engine actually submitted, so a scenario never hardcodes a venue label and
/// no implementation is forced into a naming scheme.
#[derive(Debug, Clone, PartialEq)]
pub enum OrderRef {
    Resting { strategy: StrategyId, venue: Venue },
    Taker { strategy: StrategyId, venue: Venue },
}

/// Something the world does during one cycle. A scenario is a list of these,
/// so a test states what the venue and the ports did rather than calling the
/// engine directly.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// The venue matches `qty` against a single named order. Quantities on the
    /// wire are cumulative, so repeating this action does not double-count.
    Fill { order: OrderRef, qty: Qty },
    /// The venue nets several of the engine's own orders into one match and
    /// reports only the total, naming the orders it covered.
    ///
    /// `at_offset` shifts the reported match instant relative to the current
    /// cycle clock, which is how a scenario lands an observation exactly on an
    /// attribution window boundary.
    BlockFill {
        orders: Vec<OrderRef>,
        qty: Qty,
        price: Price,
        at_offset: i64,
    },
    /// Feed lag or lead: subsequent fill events report a match instant shifted
    /// by this many seconds from the cycle clock.
    SetFillEventTimeOffset { seconds: i64 },
    /// Introduce a candidate part-way through a run.
    AddCandidate { candidate: StrategyCandidate },
    /// Advance the clock without running a cycle.
    AdvanceSeconds { seconds: i64 },
    /// Add inventory mid-run, so a scenario can starve the engine and then
    /// relieve it.
    CreditInstrument { instrument: InstrumentId, qty: Qty },
    /// Pin the inventory *read view* at its current values while the true
    /// inventory keeps moving. An engine that trusts the source instead of its
    /// own committed quantities keeps sizing against stale numbers.
    FreezeInventory,
    /// Let inventory reads see the true inventory again.
    UnfreezeInventory,
    /// Stop delivering fill events. The fills still happened at the venue, so
    /// this separates what the engine was told from what is true.
    WithholdFillEvents,
    /// Resume delivering fill events, including everything withheld.
    ReleaseFillEvents,
    /// Replay every fill event from the beginning, ignoring the cursor. Events
    /// carry cumulative quantities, so a consumer that adds them up inflates its
    /// position while one that takes a maximum is unaffected.
    RedeliverAllFillEvents,
    /// Deliver every block already reported once more. Blocks are idempotent on
    /// `block_ref`, so a correct consumer counts each one once.
    RedeliverAllBlocks,
    /// The next submission on `venue` reaches the venue and creates an order,
    /// but the acknowledgement is lost. Like `LoseNextAcknowledgement`, for one
    /// venue only, so a scenario can lose the second leg of a pair.
    LoseNextAcknowledgementOn { venue: Venue },
    /// The next submission reaches the venue and creates an order, but the
    /// acknowledgement is lost. The engine must not conclude the order does not
    /// exist; the venue deduplicates on client reference, so a correct retry
    /// reattaches to the same order instead of creating a second one.
    LoseNextAcknowledgement,
    /// The next inventory read returns a port error.
    FailNextInventoryRead,
    /// The next fill-event poll returns a port error. The cursor must not
    /// advance past events that were never delivered.
    FailNextFillPoll,
    /// The next cancel returns a port error, leaving the order live at the venue.
    FailNextCancel,
    /// The venue rejects the next taker submission outright, creating no order.
    RejectNextTakerSubmission,
    /// Stop auto-filling taker orders on acceptance, so a scenario can leave a
    /// taker unfilled and control its fills explicitly.
    DisableTakerAutofill,
    /// Resume auto-filling taker orders on acceptance.
    EnableTakerAutofill,
    /// Move the instrument's best ask mid-run, changing what taker orders price
    /// against for subsequent cycles.
    SetBestAsk { instrument: InstrumentId, price: Price },
    /// Discard the engine and build a new one against the same world before this
    /// cycle runs. Only the persisted snapshot survives, so anything the engine
    /// kept solely in memory is gone.
    Restart,
    /// The venue hands the next order it accepts the identifier of an order it
    /// has already closed. Identifiers are recycled once an order's attribution
    /// window has passed, so a scenario advances the clock first.
    RecycleClosedOrderId { from: OrderRef },
}

/// One cycle of the run: the actions the world performs before the engine's
/// cycle executes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Step {
    pub actions: Vec<Action>,
}

impl Step {
    /// A cycle in which the world does nothing. Still a full engine cycle, so
    /// idle steps are how a scenario tests that repeated cycles are idempotent.
    pub fn idle() -> Self {
        Self::default()
    }

    /// A cycle in which the world performs `actions`, in the order given.
    pub fn with(actions: Vec<Action>) -> Self {
        Self { actions }
    }
}

/// A complete run description: the starting world, and one `Step` per cycle.
/// Built with the chained methods below and handed to [`run`].
#[derive(Debug, Clone)]
pub struct Scenario {
    pub config: EngineConfig,
    pub candidates: Vec<StrategyCandidate>,
    pub initial_inventory: Vec<(InstrumentId, Qty)>,
    pub best_asks: Vec<(InstrumentId, Price)>,
    pub start: Timestamp,
    pub cycle_seconds: i64,
    pub steps: Vec<Step>,
}

impl Scenario {
    /// A scenario with default engine configuration, no inventory, no quotes
    /// and no steps. Every other builder method adds to this.
    pub fn new(candidates: Vec<StrategyCandidate>) -> Self {
        Self {
            config: EngineConfig::default(),
            candidates,
            initial_inventory: Vec::new(),
            best_asks: Vec::new(),
            start: Timestamp::from_seconds(1_000_000),
            cycle_seconds: 10,
            steps: Vec::new(),
        }
    }

    /// Credit the instrument with `qty` before the first cycle. Inventory is
    /// the shared resource strategies compete for, so this is what makes a
    /// scenario over- or under-subscribed.
    pub fn inventory(mut self, instrument_id: InstrumentId, qty: Qty) -> Self {
        self.initial_inventory.push((instrument_id, qty));
        self
    }

    /// Set the instrument's best ask, which is what taker orders price against.
    pub fn ask(mut self, instrument_id: InstrumentId, price: Price) -> Self {
        self.best_asks.push((instrument_id, price));
        self
    }

    /// Seconds the clock advances between cycles. Zero holds the clock still, so
    /// orders accepted in different cycles share a creation instant and are
    /// separated only by the venue tick that accepted them.
    pub fn with_cycle_seconds(mut self, seconds: i64) -> Self {
        self.cycle_seconds = seconds;
        self
    }

    /// Append one cycle in which the world performs `actions`.
    pub fn step(mut self, actions: Vec<Action>) -> Self {
        self.steps.push(Step::with(actions));
        self
    }

    /// Append `count` cycles in which the world does nothing.
    pub fn idle_steps(mut self, count: usize) -> Self {
        for _ in 0..count {
            self.steps.push(Step::idle());
        }
        self
    }
}

/// Everything observable after a run: what the venue saw, and what the engine
/// reports about itself.
///
/// `attempts`, `venue_orders`, `cancelled_order_ids`, `snapshot` and
/// `inventory` come from the fake world; `positions` and `pending_orders` come
/// from the engine's public accessors. Assertions are written against the world
/// wherever possible, so the engine cannot pass by agreeing with itself.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub attempts: Vec<SubmitAttempt>,
    pub venue_orders: Vec<VenueOrder>,
    pub cancelled_order_ids: Vec<OrderId>,
    pub positions: Vec<Position>,
    pub pending_orders: Vec<PendingOrder>,
    pub snapshot: Option<Snapshot>,
    pub cycle_errors: Vec<String>,
    pub inventory: Vec<(InstrumentId, Qty)>,
}

impl Outcome {
    /// Every immediate-or-cancel order reaching the venue, in submission order.
    pub fn taker_orders(&self) -> Vec<&VenueOrder> {
        self.venue_orders
            .iter()
            .filter(|order| order.kind == OrderKind::ImmediateOrCancel)
            .collect()
    }

    /// Every resting limit order reaching the venue, in submission order.
    pub fn resting_orders(&self) -> Vec<&VenueOrder> {
        self.venue_orders
            .iter()
            .filter(|order| order.kind == OrderKind::RestingLimit)
            .collect()
    }

    /// Taker orders one strategy placed on one venue. Length is the load-bearing
    /// property in duplicate-suppression cases: a second entry here means the
    /// engine submitted twice, whatever it believes about its own state.
    pub fn takers_for(&self, strategy_id: &str, venue: Venue) -> Vec<&VenueOrder> {
        self.taker_orders()
            .into_iter()
            .filter(|order| order.strategy_id.as_str() == strategy_id && order.venue == venue)
            .collect()
    }

    /// Taker orders on one instrument across all strategies and venues.
    pub fn takers_on_instrument(&self, instrument_id: &str) -> Vec<&VenueOrder> {
        self.taker_orders()
            .into_iter()
            .filter(|order| order.instrument_id.as_str() == instrument_id)
            .collect()
    }

    /// Total taker size committed against one instrument. This is the quantity
    /// that must not exceed available inventory: over-commitment shows up here
    /// even when each individual order looks reasonable on its own.
    pub fn taker_qty_on_instrument(&self, instrument_id: &str) -> Qty {
        self.takers_on_instrument(instrument_id)
            .into_iter()
            .map(|order| order.size)
            .sum()
    }

    /// Resting orders one strategy placed on one venue.
    pub fn resting_for(&self, strategy_id: &str, venue: Venue) -> Vec<&VenueOrder> {
        self.resting_orders()
            .into_iter()
            .filter(|order| order.strategy_id.as_str() == strategy_id && order.venue == venue)
            .collect()
    }

    /// The engine's reported position for one strategy, or `None` if it holds
    /// none. Absence and a zero position are different answers.
    pub fn position(&self, strategy_id: &str) -> Option<&Position> {
        self.positions
            .iter()
            .find(|position| position.strategy_id.as_str() == strategy_id)
    }

    /// Orders the engine still considers outstanding for one strategy and venue.
    /// A pending entry that the venue never accepted, or that the venue has
    /// already filled, is a reconciliation failure.
    pub fn pending_for(&self, strategy_id: &str, venue: Venue) -> Vec<&PendingOrder> {
        self.pending_orders
            .iter()
            .filter(|order| order.strategy_id.as_str() == strategy_id && order.venue == venue)
            .collect()
    }

    /// Inventory remaining for one instrument at the end of the run, from the
    /// world rather than the engine. Zero if the instrument was never credited.
    pub fn inventory_of(&self, instrument_id: &str) -> Qty {
        self.inventory
            .iter()
            .find(|(id, _)| id.as_str() == instrument_id)
            .map(|(_, qty)| *qty)
            .unwrap_or(0)
    }
}

#[derive(Default)]
/// The whole mutable world behind [`FakeWorld`]: venue books, the fill and block
/// feeds, the injected failure counters, and the persisted snapshot. Private, so
/// a test reaches it only through the actions and accessors above.
struct WorldState {
    now_millis: i64,
    inventory: BTreeMap<InstrumentId, Qty>,
    frozen_view: Option<BTreeMap<InstrumentId, Qty>>,
    best_asks: BTreeMap<InstrumentId, Price>,
    candidates: Vec<StrategyCandidate>,
    orders: Vec<VenueOrder>,
    order_index_by_client_ref: BTreeMap<ClientRef, usize>,
    attempts: Vec<SubmitAttempt>,
    cancelled_order_ids: Vec<OrderId>,
    events: Vec<FillEvent>,
    blocks: Vec<BlockFill>,
    events_withheld: bool,
    redeliver_all: bool,
    redeliver_blocks: bool,
    event_time_offset_seconds: i64,
    lose_next_ack: usize,
    lose_next_ack_on: Vec<Venue>,
    inventory_failures: usize,
    poll_failures: usize,
    cancel_failures: usize,
    taker_rejections: usize,
    taker_autofill: bool,
    next_order_id: u64,
    next_seq: u64,
    /// The current venue tick. Every order accepted in a tick shares it, so the
    /// sequence number cannot stand in for a unique per-order identity.
    venue_tick: u64,
    next_block_seq: u64,
    saved: Option<Snapshot>,
    /// Identifier the next accepted order will be given instead of a fresh one.
    recycle_next: Option<OrderId>,
}

/// The fake venue and the six injected ports, sharing one interior-mutable
/// world. One instance backs every port the engine is constructed with, so the
/// engine sees a single consistent world across `Clock`, `StrategySource`,
/// `InventorySource`, `OrderGateway`, `FillEventSource` and `StateStore`.
pub struct FakeWorld {
    state: RefCell<WorldState>,
}

impl FakeWorld {
    /// Build the starting world from a scenario's inventory, quotes, candidates
    /// and start instant. Taker autofill begins enabled.
    pub fn new(scenario: &Scenario) -> Self {
        let mut state = WorldState {
            now_millis: scenario.start.as_millis(),
            taker_autofill: true,
            candidates: scenario.candidates.clone(),
            ..WorldState::default()
        };
        for (instrument_id, qty) in &scenario.initial_inventory {
            state.inventory.insert(instrument_id.clone(), *qty);
        }
        for (instrument_id, price) in &scenario.best_asks {
            state.best_asks.insert(instrument_id.clone(), *price);
        }
        Self {
            state: RefCell::new(state),
        }
    }

    /// Move the virtual clock forward. Time only ever advances explicitly, so a
    /// scenario replays identically.
    pub fn advance_seconds(&self, seconds: i64) {
        self.state.borrow_mut().now_millis += seconds * 1_000;
    }

    /// One venue tick per cycle. Every order accepted during the tick shares its
    /// sequence number.
    pub fn begin_tick(&self) {
        self.state.borrow_mut().venue_tick += 1;
    }

    /// Perform one scenario action against the world. `Action::Restart` is
    /// handled by [`run`] instead, because it replaces the engine rather than
    /// changing the world.
    pub fn apply(&self, action: &Action) {
        match action {
            Action::Fill { order, qty } => self.fill(order, *qty),
            Action::CreditInstrument { instrument, qty } => {
                let mut state = self.state.borrow_mut();
                *state.inventory.entry(instrument.clone()).or_insert(0) += qty;
            }
            Action::BlockFill {
                orders,
                qty,
                price,
                at_offset,
            } => self.block_fill(orders, *qty, *price, *at_offset),
            Action::SetFillEventTimeOffset { seconds } => {
                self.state.borrow_mut().event_time_offset_seconds = *seconds;
            }
            Action::AddCandidate { candidate } => {
                self.state.borrow_mut().candidates.push(candidate.clone());
            }
            Action::AdvanceSeconds { seconds } => {
                self.advance_seconds(*seconds);
            }
            Action::RedeliverAllBlocks => {
                self.state.borrow_mut().redeliver_blocks = true;
            }
            Action::FreezeInventory => {
                let mut state = self.state.borrow_mut();
                let view = state.inventory.clone();
                state.frozen_view = Some(view);
            }
            Action::UnfreezeInventory => {
                self.state.borrow_mut().frozen_view = None;
            }
            Action::WithholdFillEvents => {
                self.state.borrow_mut().events_withheld = true;
            }
            Action::ReleaseFillEvents => {
                self.state.borrow_mut().events_withheld = false;
            }
            Action::RedeliverAllFillEvents => {
                self.state.borrow_mut().redeliver_all = true;
            }
            Action::LoseNextAcknowledgement => {
                self.state.borrow_mut().lose_next_ack += 1;
            }
            Action::LoseNextAcknowledgementOn { venue } => {
                self.state.borrow_mut().lose_next_ack_on.push(*venue);
            }
            Action::FailNextInventoryRead => {
                self.state.borrow_mut().inventory_failures += 1;
            }
            Action::FailNextFillPoll => {
                self.state.borrow_mut().poll_failures += 1;
            }
            Action::FailNextCancel => {
                self.state.borrow_mut().cancel_failures += 1;
            }
            Action::RejectNextTakerSubmission => {
                self.state.borrow_mut().taker_rejections += 1;
            }
            Action::DisableTakerAutofill => {
                self.state.borrow_mut().taker_autofill = false;
            }
            Action::EnableTakerAutofill => {
                self.state.borrow_mut().taker_autofill = true;
            }
            Action::SetBestAsk { instrument, price } => {
                self.state
                    .borrow_mut()
                    .best_asks
                    .insert(instrument.clone(), *price);
            }
            Action::Restart => {}
            Action::RecycleClosedOrderId { from } => {
                let mut state = self.state.borrow_mut();
                if let Some(index) = Self::resolve_closed(&state, from) {
                    let label = state.orders[index].order_id.clone();
                    state.recycle_next = Some(label);
                }
            }
        }
    }

    /// Like `resolve`, but for an order the venue has already closed.
    fn resolve_closed(state: &WorldState, reference: &OrderRef) -> Option<usize> {
        let (strategy_id, venue, kind) = match reference {
            OrderRef::Resting { strategy: strategy_id, venue } => (strategy_id, venue, OrderKind::RestingLimit),
            OrderRef::Taker { strategy: strategy_id, venue } => (strategy_id, venue, OrderKind::ImmediateOrCancel),
        };
        state.orders.iter().position(|order| {
            &order.strategy_id == strategy_id && order.venue == *venue && order.kind == kind && !order.live
        })
    }

    fn resolve(state: &WorldState, reference: &OrderRef) -> Option<usize> {
        let (strategy_id, venue, kind) = match reference {
            OrderRef::Resting {
                strategy: strategy_id,
                venue,
            } => (strategy_id, venue, OrderKind::RestingLimit),
            OrderRef::Taker {
                strategy: strategy_id,
                venue,
            } => (strategy_id, venue, OrderKind::ImmediateOrCancel),
        };
        state.orders.iter().position(|order| {
            &order.strategy_id == strategy_id
                && order.venue == *venue
                && order.kind == kind
                && order.live
        })
    }

    fn fill(&self, reference: &OrderRef, qty: Qty) {
        let mut state = self.state.borrow_mut();
        let Some(index) = Self::resolve(&state, reference) else {
            return;
        };
        Self::fill_index(&mut state, index, qty);
    }

    fn fill_index(state: &mut WorldState, index: usize, qty: Qty) {
        let remaining = state.orders[index].size - state.orders[index].filled_qty;
        let applied = if qty < remaining { qty } else { remaining };
        if applied <= 0 {
            return;
        }
        state.orders[index].filled_qty += applied;
        if state.orders[index].filled_qty >= state.orders[index].size {
            state.orders[index].live = false;
        }
        let instrument_id = state.orders[index].instrument_id.clone();
        *state.inventory.entry(instrument_id.clone()).or_insert(0) += applied;

        state.next_seq += 1;
        let at_millis = state.now_millis + state.event_time_offset_seconds * 1_000;
        let event = FillEvent {
            seq: state.next_seq,
            order_id: state.orders[index].order_id.clone(),
            client_ref: state.orders[index].client_ref.clone(),
            strategy_id: state.orders[index].strategy_id.clone(),
            instrument_id,
            venue: state.orders[index].venue,
            cumulative_filled_qty: state.orders[index].filled_qty,
            price: state.orders[index].limit_price,
            at: Timestamp::from_millis(at_millis),
        };
        state.events.push(event);
    }

    /// The venue nets several of the engine's own orders into one match and
    /// reports only the total. No per-order fill event is emitted, and every
    /// covered order is closed by the venue whether or not it filled.
    fn block_fill(&self, references: &[OrderRef], qty: Qty, price: Price, at_offset: i64) {
        let mut state = self.state.borrow_mut();
        let mut indices: Vec<usize> = Vec::new();
        for reference in references {
            if let Some(index) = Self::resolve(&state, reference) {
                indices.push(index);
            }
        }
        if indices.is_empty() {
            return;
        }
        let instrument_id = state.orders[indices[0]].instrument_id.clone();
        let venue = state.orders[indices[0]].venue;
        let covers: Vec<OrderId> = indices
            .iter()
            .map(|index| state.orders[*index].order_id.clone())
            .collect();
        for index in &indices {
            state.orders[*index].live = false;
        }
        *state.inventory.entry(instrument_id.clone()).or_insert(0) += qty;

        state.next_block_seq += 1;
        state.next_seq += 1;
        let seq = state.next_seq;
        let block_ref = BlockRef::new(format!("block-{}", state.next_block_seq));
        let at_millis = state.now_millis + at_offset * 1_000;
        state.blocks.push(BlockFill {
            seq,
            block_ref,
            instrument_id,
            venue,
            covers,
            block_qty: qty,
            price,
            at: Timestamp::from_millis(at_millis),
        });
    }

    /// Every submission the engine issued, in order, including ones the venue
    /// deduplicated or rejected.
    pub fn attempts(&self) -> Vec<SubmitAttempt> {
        self.state.borrow().attempts.clone()
    }

    /// Orders that exist at the venue, in the order the venue created them.
    pub fn orders(&self) -> Vec<VenueOrder> {
        self.state.borrow().orders.clone()
    }

    /// Identifiers the engine successfully cancelled, in cancellation order.
    /// Repeats are visible, which is how a scenario catches a cancel reissued
    /// for an order the venue already closed.
    pub fn cancelled_order_ids(&self) -> Vec<OrderId> {
        self.state.borrow().cancelled_order_ids.clone()
    }

    /// The last snapshot the engine persisted, kept across engine instances so a
    /// restart resumes from real durable state rather than a fresh fake.
    pub fn saved(&self) -> Option<Snapshot> {
        self.state.borrow().saved.clone()
    }

    /// The world's true remaining inventory, unaffected by a frozen read view.
    pub fn inventory(&self) -> Vec<(InstrumentId, Qty)> {
        self.state
            .borrow()
            .inventory
            .iter()
            .map(|(instrument_id, qty)| (instrument_id.clone(), *qty))
            .collect()
    }
}

impl StrategySource for FakeWorld {
    fn candidates(&self) -> Result<Vec<StrategyCandidate>, PortError> {
        Ok(self.state.borrow().candidates.clone())
    }
}

impl InventorySource for FakeWorld {
    fn instrument_inventory(&self, instrument_id: &InstrumentId) -> Result<Qty, PortError> {
        let mut state = self.state.borrow_mut();
        if state.inventory_failures > 0 {
            state.inventory_failures -= 1;
            return Err(PortError::Unavailable(String::from("inventory read failed")));
        }
        let source = match &state.frozen_view {
            Some(view) => view,
            None => &state.inventory,
        };
        Ok(source.get(instrument_id).copied().unwrap_or(0))
    }

    fn settlement_balance(&self) -> Result<Price, PortError> {
        Ok(1_000_000.0)
    }
}

impl OrderGateway for FakeWorld {
    fn submit(&self, request: &OrderRequest) -> Result<OrderAccepted, SubmitError> {
        let mut state = self.state.borrow_mut();

        if let Some(index) = state
            .order_index_by_client_ref
            .get(&request.client_ref)
            .copied()
        {
            let order_id = state.orders[index].order_id.clone();
            state.attempts.push(SubmitAttempt {
                client_ref: request.client_ref.clone(),
                strategy_id: request.strategy_id.clone(),
                instrument_id: request.instrument_id.clone(),
                venue: request.venue,
                kind: request.kind,
                limit_price: request.limit_price,
                size: request.size,
                outcome: SubmitOutcome::Deduplicated(order_id.clone()),
            });
            let venue_seq = state.orders[index].venue_seq;
            // The answer to a retry can be lost as well.
            let ack_lost = state.lose_next_ack > 0;
            if ack_lost {
                state.lose_next_ack -= 1;
                if let Some(last) = state.attempts.last_mut() {
                    last.outcome = SubmitOutcome::AcknowledgementLost(order_id.clone());
                }
                return Err(SubmitError::AcknowledgementLost);
            }
            return Ok(OrderAccepted {
                order_id,
                client_ref: request.client_ref.clone(),
                venue_seq,
            });
        }

        if request.kind == OrderKind::ImmediateOrCancel && state.taker_rejections > 0 {
            state.taker_rejections -= 1;
            state.attempts.push(SubmitAttempt {
                client_ref: request.client_ref.clone(),
                strategy_id: request.strategy_id.clone(),
                instrument_id: request.instrument_id.clone(),
                venue: request.venue,
                kind: request.kind,
                limit_price: request.limit_price,
                size: request.size,
                outcome: SubmitOutcome::Rejected,
            });
            return Err(SubmitError::Rejected("venue rejected taker order".into()));
        }

        state.next_order_id += 1;
        let order_id = match state.recycle_next.take() {
            Some(label) => label,
            None => OrderId::new(venue_label(state.next_order_id)),
        };
        let venue_seq = state.venue_tick;
        let index = state.orders.len();
        state.orders.push(VenueOrder {
            order_id: order_id.clone(),
            client_ref: request.client_ref.clone(),
            strategy_id: request.strategy_id.clone(),
            instrument_id: request.instrument_id.clone(),
            venue: request.venue,
            kind: request.kind,
            limit_price: request.limit_price,
            size: request.size,
            filled_qty: 0,
            venue_seq,
            live: true,
            cancelled: false,
        });
        state
            .order_index_by_client_ref
            .insert(request.client_ref.clone(), index);

        if request.kind == OrderKind::ImmediateOrCancel && state.taker_autofill {
            let size = request.size;
            Self::fill_index(&mut state, index, size);
        }

        let mut ack_lost = state.lose_next_ack > 0;
        if ack_lost {
            state.lose_next_ack -= 1;
        } else if let Some(position) = state.lose_next_ack_on.iter().position(|venue| *venue == request.venue) {
            state.lose_next_ack_on.remove(position);
            ack_lost = true;
        }

        state.attempts.push(SubmitAttempt {
            client_ref: request.client_ref.clone(),
            strategy_id: request.strategy_id.clone(),
            instrument_id: request.instrument_id.clone(),
            venue: request.venue,
            kind: request.kind,
            limit_price: request.limit_price,
            size: request.size,
            outcome: if ack_lost {
                SubmitOutcome::AcknowledgementLost(order_id.clone())
            } else {
                SubmitOutcome::Accepted(order_id.clone())
            },
        });

        if ack_lost {
            return Err(SubmitError::AcknowledgementLost);
        }

        Ok(OrderAccepted {
            order_id,
            client_ref: request.client_ref.clone(),
            venue_seq,
        })
    }

    fn cancel(&self, order_id: &OrderId) -> Result<(), PortError> {
        let mut state = self.state.borrow_mut();
        if state.cancel_failures > 0 {
            state.cancel_failures -= 1;
            return Err(PortError::Transport(String::from("cancel failed")));
        }
        state.cancelled_order_ids.push(order_id.clone());
        let live_match = state
            .orders
            .iter()
            .position(|order| &order.order_id == order_id && order.live);
        if let Some(index) = live_match.or_else(|| {
            state
                .orders
                .iter()
                .position(|order| &order.order_id == order_id)
        }) {
            state.orders[index].cancelled = true;
            state.orders[index].live = false;
        }
        Ok(())
    }

    fn open_orders(&self) -> Result<Vec<OpenOrder>, PortError> {
        let state = self.state.borrow();
        Ok(state
            .orders
            .iter()
            // An immediate-or-cancel order is matched at submission and never
            // rests, so it is never live at the venue.
            .filter(|order| order.live && order.kind == OrderKind::RestingLimit)
            .map(|order| OpenOrder {
                order_id: order.order_id.clone(),
                client_ref: order.client_ref.clone(),
                strategy_id: order.strategy_id.clone(),
                instrument_id: order.instrument_id.clone(),
                venue: order.venue,
                kind: order.kind,
                limit_price: order.limit_price,
                size: order.size,
                filled_qty: order.filled_qty,
                venue_seq: order.venue_seq,
            })
            .collect())
    }

    fn best_offer(&self, instrument_id: &InstrumentId) -> Result<Option<Price>, PortError> {
        Ok(self.state.borrow().best_asks.get(instrument_id).copied())
    }
}

impl FillEventSource for FakeWorld {
    fn poll(&self, cursor: &Cursor) -> Result<FillBatch, PortError> {
        let mut state = self.state.borrow_mut();
        if state.poll_failures > 0 {
            state.poll_failures -= 1;
            return Err(PortError::Transport(String::from("fill stream failed")));
        }
        let position = cursor.as_str().parse::<u64>().unwrap_or(0);
        if state.events_withheld {
            return Ok(FillBatch {
                events: Vec::new(),
                blocks: Vec::new(),
                next_cursor: cursor.clone(),
            });
        }
        let from = if state.redeliver_all { 0 } else { position };
        state.redeliver_all = false;
        let block_from = if state.redeliver_blocks { 0 } else { position };
        state.redeliver_blocks = false;
        let events: Vec<FillEvent> = state
            .events
            .iter()
            .filter(|event| event.seq > from)
            .cloned()
            .collect();
        let blocks: Vec<BlockFill> = state
            .blocks
            .iter()
            .filter(|block| block.seq > block_from)
            .cloned()
            .collect();
        let highest = state
            .events
            .iter()
            .map(|event| event.seq)
            .chain(state.blocks.iter().map(|block| block.seq))
            .max()
            .unwrap_or(0);
        let next = if highest > position {
            highest
        } else {
            position
        };
        Ok(FillBatch {
            events,
            blocks,
            next_cursor: Cursor::new(next.to_string()),
        })
    }
}

impl StateStore for FakeWorld {
    fn load(&self) -> Result<Option<Snapshot>, PortError> {
        Ok(self.state.borrow().saved.clone())
    }

    fn save(&self, snapshot: &Snapshot) -> Result<(), PortError> {
        self.state.borrow_mut().saved = Some(snapshot.clone());
        Ok(())
    }
}

impl Clock for FakeWorld {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.state.borrow().now_millis)
    }
}

/// Execute a scenario and return everything observable afterwards.
///
/// One `Step` is one engine cycle: the world ticks, the step's actions are
/// applied, the engine runs a full cycle, then the clock advances. A cycle that
/// returns an error does not abort the run — the error is recorded in
/// `Outcome::cycle_errors` and the following cycles still execute, so a test can
/// require that the engine recovers from a port failure rather than merely
/// surviving one. `Action::Restart` rebuilds the engine against the same world,
/// which keeps the persisted snapshot but discards all in-memory state.
pub fn run(scenario: Scenario) -> Outcome {
    let world = FakeWorld::new(&scenario);
    let mut cycle_errors = Vec::new();
    let mut engine = Engine::new(
        scenario.config.clone(),
        &world,
        &world,
        &world,
        &world,
        &world,
        &world,
    );

    for step in &scenario.steps {
        world.begin_tick();
        let mut restart_requested = false;
        for action in &step.actions {
            if action == &Action::Restart {
                restart_requested = true;
            } else {
                world.apply(action);
            }
        }
        if restart_requested {
            engine = Engine::new(
                scenario.config.clone(),
                &world,
                &world,
                &world,
                &world,
                &world,
                &world,
            );
        }
        if let Err(error) = engine.run_cycle() {
            cycle_errors.push(error.to_string());
        }
        world.advance_seconds(scenario.cycle_seconds);
    }

    let positions = engine.positions();
    let pending_orders = engine.pending_orders();

    let outcome = Outcome {
        attempts: world.attempts(),
        venue_orders: world.orders(),
        cancelled_order_ids: world.cancelled_order_ids(),
        positions,
        pending_orders,
        snapshot: world.saved(),
        cycle_errors,
        inventory: world.inventory(),
    };
    outcome
}

/// A two-legged strategy candidate: the instrument and price it takes on the
/// north venue, the one it rests on the south venue, and the depth available on
/// each. The expiry is far past the end of any scenario, so a candidate stops
/// being eligible only for reasons the test creates.
pub fn candidate(
    strategy_id: &str,
    north_instrument: &str,
    south_instrument: &str,
    north_price: Price,
    south_price: Price,
    north_depth: Qty,
    south_depth: Qty,
) -> StrategyCandidate {
    StrategyCandidate::new(
        strategy(strategy_id),
        instrument(north_instrument),
        instrument(south_instrument),
        north_price,
        south_price,
        north_depth,
        south_depth,
        Timestamp::from_seconds(1_000_000 + 86_400),
    )
}

/// The venue fills `qty` of the resting order this strategy placed on `venue`.
/// The order is resolved at run time, so the scenario never names an identifier.
pub fn fill_resting(strategy_id: &str, venue: Venue, qty: Qty) -> Action {
    Action::Fill {
        order: OrderRef::Resting {
            strategy: strategy(strategy_id),
            venue,
        },
        qty,
    }
}

/// The venue fills `qty` of the taker order this strategy placed on `venue`.
pub fn fill_taker(strategy_id: &str, venue: Venue, qty: Qty) -> Action {
    Action::Fill {
        order: OrderRef::Taker {
            strategy: strategy(strategy_id),
            venue,
        },
        qty,
    }
}

/// A single venue match netting several of the engine's orders on one instrument
/// into one report. The venue gives `qty` as the total and names the orders it
/// covered; it never says how the total divides.
///
/// `at_offset` shifts the reported match instant relative to the current cycle
/// clock, which is how a scenario lands an observation exactly on an attribution
/// window boundary.
pub fn block_orders(orders: Vec<OrderRef>, qty: Qty, price: Price, at_offset: i64) -> Action {
    Action::BlockFill {
        orders,
        qty,
        price,
        at_offset,
    }
}

/// Taker-order references for several strategies on one venue, in the order
/// given. Block reports name their covered orders in this order.
pub fn taker_refs(strategies: &[&str], venue: Venue) -> Vec<OrderRef> {
    strategies
        .iter()
        .map(|strategy_id| OrderRef::Taker {
            strategy: strategy(strategy_id),
            venue,
        })
        .collect()
}

/// A single venue match netting several taker orders on one instrument. The
/// venue reports `qty` as the total and names the covered orders; it never says
/// how the total divides.
pub fn block_takers(strategies: &[&str], venue: Venue, qty: Qty, price: Price) -> Action {
    block_orders(taker_refs(strategies, venue), qty, price, 0)
}

/// The same match reported with a venue timestamp displaced by `at_offset`
/// seconds from the cycle in which the engine observes it. The feed lags and
/// leads; the attribution window is tested against the venue timestamp.
pub fn block_takers_at(
    strategies: &[&str],
    venue: Venue,
    qty: Qty,
    price: Price,
    at_offset: i64,
) -> Action {
    block_orders(taker_refs(strategies, venue), qty, price, at_offset)
}

/// A seeded linear-congruential generator. Deterministic and seed-addressed, so
/// randomised scenarios replay identically across runs and machines.
pub struct Prng {
    state: u64,
}

impl Prng {
    /// Seed the generator. The same seed always yields the same sequence.
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1),
        }
    }

    /// The next value in the sequence.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state >> 11
    }

    /// A value in `0..bound`, or 0 when `bound` is zero.
    pub fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next_u64() % bound as u64) as usize
        }
    }
}
