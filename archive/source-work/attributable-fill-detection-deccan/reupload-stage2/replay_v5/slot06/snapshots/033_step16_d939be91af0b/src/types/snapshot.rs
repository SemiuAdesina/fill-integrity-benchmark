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

/// One entry order this engine placed for one leg of one mandate.
///
/// The engine's own key for a leg is the `client_ref` it chose: it is the only
/// identity it always knows, and the one the venue deduplicates on. `order_id`
/// is therefore optional and learned later — from the acknowledgement, from a
/// later observation carrying the client reference, or from the live order list.
///
/// The closure state lives here rather than in a set keyed by venue identifier,
/// because the venue recycles identifiers once a window has closed: state keyed
/// by identifier would follow the identifier onto an unrelated later order.
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
    /// Start of the order's attribution window. Set once, at the first attempt
    /// to submit it: a retry after a lost acknowledgement does not reopen it.
    pub window_start: Timestamp,
    /// This engine has decided the leg must be closed.
    pub closing: bool,
    /// This engine cancelled the order at the venue.
    pub cancelled: bool,
    /// The venue closed the order itself, by naming it in a block.
    pub venue_closed: bool,
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
    /// Engine clock at the instant this order began being tracked.
    pub window_start: Timestamp,
    pub venue_seq: Option<u64>,
    pub size: Qty,
    /// Quantity attributed to this order by per-order fill events.
    pub direct_filled: Qty,
    /// Quantity attributed to this order by block matches.
    pub block_filled: Qty,
    /// Set once a block naming this order has been observed.
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
}

/// Everything that must survive a restart.
///
/// `StateStore::load` returns exactly what the matching `save` wrote, so a field
/// here is durable and a field held only on `Engine` is not.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub positions: Vec<Position>,
    pub legs: Vec<LegRecord>,
    pub attempted_strategies: Vec<StrategyId>,
    pub client_seq: u64,
    pub fill_cursor: Cursor,
    pub submissions: Vec<SubmissionRecord>,
    pub absorbed_blocks: Vec<BlockRef>,
    pub neutralizations: Vec<NeutralizationRecord>,
}

/// Legacy alias kept so a snapshot field describing pending work still names a
/// leg. `PendingOrder` itself is frozen and stays what it was.
pub type PendingLegRecord = PendingOrder;
