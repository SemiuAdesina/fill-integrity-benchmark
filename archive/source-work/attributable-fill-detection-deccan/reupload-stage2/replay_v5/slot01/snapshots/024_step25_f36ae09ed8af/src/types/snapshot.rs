use crate::ports::OrderKind;
use crate::types::ids::{
    BlockRef, ClientRef, InstrumentId, OrderId, Price, Qty, StrategyId, Venue,
};
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

/// Why the engine submitted an order.
///
/// A `Leg` is one half of a mandate, resting on its venue. A `Neutralization`
/// is the corrective order that closes the exposure a one-legged mandate left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderRole {
    Leg,
    Neutralization,
}

/// How far a resting leg has got through being closed.
///
/// Correcting a resting leg cannot happen in the cycle the need is first seen:
/// the order has to be moved into closure first, and only then is whatever
/// residual remains covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosureState {
    /// Nothing asked of the venue yet.
    Open,
    /// The leg has been marked for closure; the venue has not been told.
    Marked,
}

impl Default for ClosureState {
    fn default() -> Self {
        ClosureState::Open
    }
}

/// One order this engine submitted, and the attribution state held against it.
///
/// `client_ref` is the engine's own key and is always known. `order_id` and
/// `venue_seq` come from the venue and are therefore optional: an order whose
/// acknowledgement was lost has neither until some later observation supplies
/// them.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmissionRecord {
    pub client_ref: ClientRef,
    pub order_id: Option<OrderId>,
    /// Start of this order's attribution window. It opens at the first attempt,
    /// so a resubmission after a lost acknowledgement does not reopen it.
    pub window_start: Timestamp,
    pub venue_seq: Option<u64>,
    pub size: Qty,
    /// Quantity attributed to this order by per-order fill events.
    pub direct_filled: Qty,
    /// Quantity attributed to this order by block matches.
    pub block_filled: Qty,
    /// Set once a block naming this order has been observed.
    pub finalized: bool,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    pub kind: OrderKind,
    pub role: OrderRole,
    pub limit_price: Price,
    pub expiry_time: Timestamp,
    /// True once the venue has answered a cancellation of this order.
    pub cancelled: bool,
    /// How far a resting leg has got through being closed.
    pub closure: ClosureState,
}

impl SubmissionRecord {
    pub fn attributed(&self) -> Qty {
        self.direct_filled + self.block_filled
    }

    /// Quantity this order could still acquire.
    ///
    /// A finalized order can acquire nothing more: the venue closed it and
    /// cancelled whatever the block did not fill.
    pub fn outstanding(&self) -> Qty {
        if self.finalized {
            return 0;
        }
        let remaining = self.size - self.attributed();
        if remaining > 0 {
            remaining
        } else {
            0
        }
    }

    /// Whether a venue match timestamp falls inside this order's window.
    ///
    /// The window is half-open: `[window_start, window_start + W)`.
    pub fn in_window(&self, at: Timestamp) -> bool {
        at >= self.window_start && at < self.window_start + crate::types::pending_order::ATTRIBUTION_WINDOW
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
}
