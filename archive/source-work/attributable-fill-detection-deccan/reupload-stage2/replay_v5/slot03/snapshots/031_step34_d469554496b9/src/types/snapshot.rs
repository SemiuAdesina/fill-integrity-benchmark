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

/// One corrective order the engine has decided to send, or has sent.
///
/// The record exists from the moment the engine commits to the order, before the
/// venue has said anything about it, so `order_id` is optional: a submission whose
/// acknowledgement was lost has a `client_ref` and no id. A record with no id is
/// therefore *potentially working* at the venue and counts for its full size until
/// its identity is recovered.
#[derive(Debug, Clone, PartialEq)]
pub struct NeutralizationRecord {
    pub strategy_id: StrategyId,
    pub venue: Venue,
    pub instrument_id: InstrumentId,
    pub client_ref: ClientRef,
    pub order_id: Option<OrderId>,
    pub limit_price: Price,
    pub size: Qty,
    pub expiry_time: Timestamp,
}

/// One entry leg the engine is working, as the engine's own bookkeeping sees it.
///
/// `client_ref` is the engine's key and is always known; `order_id` and
/// `venue_seq` come from the venue and are therefore optional, because an order
/// whose acknowledgement was lost has neither until a later observation supplies
/// them.
///
/// `closing` and `cancelled` are the closure sequence for this leg. They live on
/// the leg — keyed by the engine's own reference — rather than in a set keyed by
/// `order_id`, because the venue recycles identifiers and closure state must never
/// speak for a later order that comes to wear the same label.
#[derive(Debug, Clone, PartialEq)]
pub struct LegRecord {
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    pub client_ref: ClientRef,
    pub order_id: Option<OrderId>,
    pub venue_seq: Option<u64>,
    pub limit_price: Price,
    pub size: Qty,
    pub expiry_time: Timestamp,
    /// Engine clock at the instant this leg began being tracked. It is also the
    /// start of the order's attribution window, and it never moves.
    pub created_at: Timestamp,
    /// The engine has decided this leg must be corrected and has moved it into
    /// closure. Set one cycle before the cancellation, so a fill already in flight
    /// has a cycle in which to land.
    pub closing: bool,
    /// The engine has cancelled this leg at the venue.
    pub cancelled: bool,
}

/// A leg the engine committed to but that no attempt has yet created at the
/// venue: the submission was rejected, or its transport failed, so nothing exists
/// and the exposure it was meant to take on is still open.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryPlan {
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    pub limit_price: Price,
    pub size: Qty,
    pub expiry_time: Timestamp,
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
    /// Engine clock at the instant this order began being tracked. This is the
    /// start of the attribution window and it survives a restart unchanged: a
    /// reopened window would admit an observation from outside the original one.
    pub window_start: Timestamp,
    pub venue_seq: Option<u64>,
    pub size: Qty,
    /// Quantity attributed to this order by per-order fill events and by the live
    /// order list. Both report the same cumulative figure, so this is the maximum
    /// ever observed and never a running sum.
    pub direct_filled: Qty,
    /// Quantity attributed to this order by block matches. Block shares are
    /// increments, so this one does accumulate; redelivery is prevented by
    /// `block_ref` idempotency instead.
    pub block_filled: Qty,
    /// Set once a block naming this order has been observed. The venue has closed
    /// the order and cancelled whatever the block did not fill.
    pub finalized: bool,
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

    /// Quantity this order could still acquire. A finalized order can acquire
    /// nothing more, whatever it managed to fill, so it contributes no outstanding
    /// quantity to a later block's basis and no working quantity to a residual.
    pub fn working(&self) -> Qty {
        if self.finalized {
            0
        } else {
            self.outstanding()
        }
    }
}

/// Everything that must survive a restart.
///
/// `StateStore::load` returns exactly what the matching `save` wrote, so a field
/// here is durable and a field held only on `Engine` is not.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub positions: Vec<Position>,
    /// Entry legs still being tracked, including their closure state and the
    /// identity of any whose acknowledgement was lost.
    pub legs: Vec<LegRecord>,
    /// Legs committed to but not yet created at the venue.
    pub entry_plans: Vec<EntryPlan>,
    pub attempted_strategies: Vec<StrategyId>,
    /// The client-reference sequence. A reset counter regenerates a reference the
    /// venue already knows.
    pub client_seq: u64,
    pub fill_cursor: Cursor,
    pub submissions: Vec<SubmissionRecord>,
    pub absorbed_blocks: Vec<BlockRef>,
    pub neutralizations: Vec<NeutralizationRecord>,
}
