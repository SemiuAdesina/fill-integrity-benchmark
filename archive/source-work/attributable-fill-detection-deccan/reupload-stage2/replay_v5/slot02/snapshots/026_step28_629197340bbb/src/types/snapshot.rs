use crate::types::ids::{
    BlockRef, ClientRef, InstrumentId, OrderId, Price, Qty, StrategyId, Venue,
};
use crate::types::pending_order::{PendingOrder, ATTRIBUTION_WINDOW, CANCEL_LEAD};
use crate::types::position::Position;
use crate::types::time::Timestamp;

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cursor(String);

impl Cursor {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn beginning() -> Self {
        Self(String::new())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Cursor {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Cursor {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// What an order the engine submitted is for.
///
/// An entry leg is one half of a paired mandate. A neutralization is the
/// corrective order that closes the residual exposure a one-legged mandate
/// leaves behind. The two are worked differently, so the role travels with the
/// record rather than being inferred from the venue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderRole {
    Entry,
    Neutralization,
}

/// One corrective order the engine has decided to send, or has sent.
///
/// The record exists from the moment the engine commits to the order, before the
/// venue has said anything about it. The order's own state — whether the venue
/// named it, what it has filled, whether a block closed it — lives on the
/// matching `SubmissionRecord`, which is keyed by the same `client_ref`.
#[derive(Debug, Clone, PartialEq)]
pub struct NeutralizationRecord {
    pub strategy_id: StrategyId,
    pub venue: Venue,
    pub instrument_id: InstrumentId,
    pub client_ref: ClientRef,
    pub limit_price: Price,
    pub size: Qty,
    pub expiry_time: Timestamp,
}

impl NeutralizationRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        strategy_id: StrategyId,
        venue: Venue,
        instrument_id: InstrumentId,
        client_ref: ClientRef,
        limit_price: Price,
        size: Qty,
        expiry_time: Timestamp,
    ) -> Self {
        Self {
            strategy_id,
            venue,
            instrument_id,
            client_ref,
            limit_price,
            size,
            expiry_time,
        }
    }
}

/// One order this engine submitted, and the attribution state held against it.
///
/// `client_ref` is the engine's own key and is always known, so it is the key
/// this record is stored under. `order_id` and `venue_seq` come from the venue
/// and are therefore optional: an order whose acknowledgement was lost has
/// neither until some later observation supplies them.
///
/// `window_start` is the instant the engine began tracking the order, which is
/// the instant of the *first* submission attempt. A retry under the same client
/// reference does not reopen it.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmissionRecord {
    pub client_ref: ClientRef,
    pub order_id: Option<OrderId>,
    /// Engine clock at the instant this order began being tracked.
    pub window_start: Timestamp,
    pub venue_seq: Option<u64>,
    pub size: Qty,
    /// Quantity attributed to this order by per-order fill events and by the
    /// live order list. Cumulative: the maximum ever observed, never a sum.
    pub direct_filled: Qty,
    /// Quantity attributed to this order by block matches. Additive across
    /// distinct `block_ref` values.
    pub block_filled: Qty,
    /// Set once a block naming this order has been observed.
    pub finalized: bool,
    /// What this order is for: a mandate's entry leg or a correction.
    pub role: OrderRole,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    pub limit_price: Price,
    pub expiry_time: Timestamp,
    /// The engine has moved this resting order into closure.
    pub closing: bool,
    /// The engine has cancelled this order at the venue.
    pub cancelled: bool,
    /// The engine has stopped working this order: it was settled or retired.
    pub retired: bool,
}

impl SubmissionRecord {
    pub fn attributed(&self) -> Qty {
        self.direct_filled + self.block_filled
    }

    pub fn outstanding(&self) -> Qty {
        let remaining = self.size - self.attributed();
        if remaining > 0 {
            remaining
        } else {
            0
        }
    }

    /// First instant at which an observation can be attributed to this order.
    pub fn attribution_start(&self) -> Timestamp {
        self.window_start
    }

    /// First instant at which an observation can no longer be attributed to this
    /// order. The window is half-open, so this instant is *excluded*.
    pub fn attribution_end(&self) -> Timestamp {
        self.window_start + ATTRIBUTION_WINDOW
    }

    /// Whether a venue match timestamp falls inside this order's window.
    pub fn window_contains(&self, at: Timestamp) -> bool {
        at >= self.attribution_start() && at < self.attribution_end()
    }

    pub fn should_cancel(&self, now: Timestamp) -> bool {
        let time_until_expiry = self.expiry_time - now;
        time_until_expiry.as_seconds() < CANCEL_LEAD.as_seconds() || now >= self.expiry_time
    }

    /// The order as the engine reports it publicly, once the venue has named it.
    pub fn as_pending_order(&self) -> Option<PendingOrder> {
        let order_id = self.order_id.clone()?;
        if self.retired || self.role != OrderRole::Entry {
            return None;
        }
        Some(PendingOrder::new(
            self.strategy_id.clone(),
            self.instrument_id.clone(),
            self.venue,
            order_id,
            self.client_ref.clone(),
            self.venue_seq.unwrap_or(0),
            self.limit_price,
            self.size,
            self.expiry_time,
            self.window_start,
        ))
    }
}

/// Everything that must survive a restart.
///
/// `StateStore::load` returns exactly what the matching `save` wrote, so a field
/// here is durable and a field held only on `Engine` is not.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub positions: Vec<Position>,
    pub attempted_strategies: Vec<StrategyId>,
    pub client_seq: u64,
    pub fill_cursor: Cursor,
    pub submissions: Vec<SubmissionRecord>,
    pub absorbed_blocks: Vec<BlockRef>,
    pub neutralizations: Vec<NeutralizationRecord>,
}
