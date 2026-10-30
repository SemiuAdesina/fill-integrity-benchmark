use crate::types::ids::{
    BlockRef, ClientRef, InstrumentId, OrderId, Price, Qty, StrategyId, Venue,
};
use crate::types::pending_order::PendingOrder;
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
/// acknowledgement was lost has a `client_ref` and no id.
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

/// One entry leg this engine has submitted but the venue has not acknowledged.
///
/// An acknowledgement that was lost leaves an order that may exist at the venue
/// with an identifier the engine cannot see. The `client_ref` is retained so the
/// retry goes out under the same one and the venue's idempotency resolves it onto
/// the original order: a retry under a freshly minted reference places the same
/// economic exposure a second time. Everything needed to resubmit the order is
/// kept beside it, because a restart drops all in-memory state.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryLegRecord {
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    pub client_ref: ClientRef,
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
///
/// Closure and cancellation live here rather than in state keyed by `order_id`:
/// the venue recycles identifiers once a window has closed, so durable state keyed
/// by an identifier speaks only for the order that held it while its window was
/// open. The client reference is the engine's own identity and is never reissued.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmissionRecord {
    pub client_ref: ClientRef,
    pub order_id: Option<OrderId>,
    /// Engine clock at the instant this order began being tracked. A restart must
    /// not reopen it: a reopened window admits an observation from outside the
    /// original one.
    pub window_start: Timestamp,
    pub venue_seq: Option<u64>,
    pub size: Qty,
    /// Quantity attributed to this order by per-order fill events.
    pub direct_filled: Qty,
    /// Quantity attributed to this order by block matches.
    pub block_filled: Qty,
    /// Set once a block naming this order has been observed.
    pub finalized: bool,
    /// This order has been moved into closure: it is being taken off the venue
    /// before anything is sent to cover what it leaves behind.
    pub closing: bool,
    /// This engine cancelled this order at the venue, so nothing about it is in
    /// flight.
    pub cancelled: bool,
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
}

/// Everything that must survive a restart.
///
/// `StateStore::load` returns exactly what the matching `save` wrote, so a field
/// here is durable and a field held only on `Engine` is not.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub positions: Vec<Position>,
    pub pending_orders: Vec<PendingOrder>,
    pub attempted_strategies: Vec<StrategyId>,
    pub client_seq: u64,
    pub fill_cursor: Cursor,
    pub submissions: Vec<SubmissionRecord>,
    pub absorbed_blocks: Vec<BlockRef>,
    pub neutralizations: Vec<NeutralizationRecord>,
    /// Entry legs submitted whose acknowledgement never arrived. They are retried
    /// under their own client references after a restart, and a forgotten one
    /// leaves a live order at the venue that no later observation can reach.
    pub pending_entries: Vec<EntryLegRecord>,
}
