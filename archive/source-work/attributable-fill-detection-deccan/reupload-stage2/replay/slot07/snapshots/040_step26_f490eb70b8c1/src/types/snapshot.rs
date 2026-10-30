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

/// One leg of a mandate the engine has tried to work but whose submission the
/// venue never acknowledged.
///
/// The venue may or may not have created the order, and its identifier is
/// unknown. What matters is the `client_ref`: it is the venue's idempotency key,
/// so a retry has to go out under the same one. Retrying under a freshly minted
/// reference would ask the venue for the same economic exposure a second time.
///
/// An attempt whose submission came back `Rejected` or `Transport` created
/// nothing and is dropped, so a later cycle is free to try again.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryAttempt {
    pub strategy_id: StrategyId,
    pub venue: Venue,
    pub instrument_id: InstrumentId,
    pub client_ref: ClientRef,
    pub limit_price: Price,
    pub size: Qty,
    pub expiry_time: Timestamp,
    /// Clock at the instant the attempt was made, so a leg recovered from a
    /// later observation is as old as the submission that created it.
    pub created_at: Timestamp,
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
    pub pending_orders: Vec<PendingOrder>,
    pub attempted_strategies: Vec<StrategyId>,
    pub client_seq: u64,
    pub fill_cursor: Cursor,
    pub submissions: Vec<SubmissionRecord>,
    pub absorbed_blocks: Vec<BlockRef>,
    pub closing_orders: Vec<OrderId>,
    pub replaced_orders: Vec<OrderId>,
    pub neutralizations: Vec<NeutralizationRecord>,
    pub entry_attempts: Vec<EntryAttempt>,
}
