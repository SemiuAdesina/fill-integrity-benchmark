use crate::types::ids::{
    BlockRef, ClientRef, InstrumentId, OrderId, Price, Qty, StrategyId, Venue,
};
use crate::types::pending_order::ATTRIBUTION_WINDOW;
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

/// Why the engine sent an order.
///
/// The two roles differ in how they are worked: an entry leg rests on the book
/// and is cancelled before its exposure is corrected, while a corrective order
/// is matched at submission and never rests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionRole {
    /// One half of a mandate: a resting limit order on one venue.
    Entry,
    /// A corrective order closing the exposure a one-legged mandate left.
    Neutralization,
}

/// What the venue last told us about a submission.
///
/// An unacknowledged submission is not the same as a failed one: the venue may
/// hold an order it never named, and the client reference is what lets a later
/// observation or a retry under the same reference reattach to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptState {
    /// Submitted, but no acknowledgement came back. The venue may hold the order.
    Unconfirmed,
    /// The venue named the order.
    Acknowledged,
    /// The venue refused the submission outright. Nothing was created.
    Failed,
}

/// One order this engine submitted, and everything held against it.
///
/// `client_ref` is the engine's own key and is always known. `order_id` and
/// `venue_seq` come from the venue and are therefore optional: an order whose
/// acknowledgement was lost has neither until some later observation supplies
/// them. The window is the one that opened when the engine first tried to place
/// the order, not the one that opened when a retry was answered.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmissionRecord {
    pub client_ref: ClientRef,
    pub order_id: Option<OrderId>,
    /// Instant this order began being tracked: the first attempt, not a retry.
    pub window_start: Timestamp,
    pub venue_seq: Option<u64>,
    pub size: Qty,
    /// Quantity attributed to this order by per-order fill events and by the
    /// live order list. Cumulative: the maximum ever observed, not a sum.
    pub direct_filled: Qty,
    /// Quantity attributed to this order by block matches. Additive, and made
    /// idempotent by `block_ref`.
    pub block_filled: Qty,
    /// Set once a block naming this order has been observed: the venue closed it.
    pub finalized: bool,
    pub role: SubmissionRole,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    pub limit_price: Price,
    pub expiry_time: Timestamp,
    pub attempt: AttemptState,
    /// This engine has decided the order has to come off the book. The
    /// cancellation itself happens on the next cycle, so a fill already in
    /// flight gets one in which to land and be attributed.
    pub closing: bool,
    /// This engine cancelled the order at the venue and the venue confirmed it.
    pub cancelled: bool,
    /// The order was abandoned near expiry. It is no longer worked.
    pub retired: bool,
}

impl SubmissionRecord {
    pub fn attributed(&self) -> Qty {
        self.direct_filled + self.block_filled
    }

    /// Quantity this order could still acquire, floored at zero. Finality is a
    /// separate question: see `is_closed`.
    pub fn outstanding(&self) -> Qty {
        let remaining = self.size - self.attributed();
        if remaining > 0 {
            remaining
        } else {
            0
        }
    }

    /// First instant at which an observation can no longer be attributed to
    /// this order. The window is half-open, so this instant is excluded.
    pub fn attribution_end(&self) -> Timestamp {
        self.window_start + ATTRIBUTION_WINDOW
    }

    /// Whether a venue match timestamp falls inside this order's window.
    pub fn in_window(&self, at: Timestamp) -> bool {
        at >= self.window_start && at < self.attribution_end()
    }

    /// Whether the venue has closed this order, so it can acquire nothing more
    /// and is not live.
    pub fn is_closed(&self) -> bool {
        self.finalized
    }
}

/// Everything that must survive a restart.
///
/// `StateStore::load` returns exactly what the matching `save` wrote, so a field
/// here is durable and a field held only on `Engine` is not.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub positions: Vec<Position>,
    /// Every order still being tracked: entry legs and committed corrections.
    pub submissions: Vec<SubmissionRecord>,
    pub attempted_strategies: Vec<StrategyId>,
    pub client_seq: u64,
    pub fill_cursor: Cursor,
    pub absorbed_blocks: Vec<BlockRef>,
}
