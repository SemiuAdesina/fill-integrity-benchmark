//! The durable state a restart has to be able to rebuild the engine from.
//!
//! `StateStore::load` returns exactly what the matching `save` wrote, so a field
//! here is durable and a field held only on `Engine` is not. A restart drops all
//! in-memory state while the venue and the custody account keep theirs, so
//! anything the engine needs in order not to redo settled work — or not to
//! reinterpret old evidence as new — has to live in here.

use crate::types::ids::{
    BlockRef, ClientRef, InstrumentId, OrderId, Price, Qty, StrategyId, Venue,
};
use crate::types::pending_order::{PendingOrder, CANCEL_LEAD};
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

/// One resting leg of a mandate.
///
/// The leg is identified by the `client_ref` the engine chose, because that is
/// the only key the engine always knows: the venue's `order_id` is unknown until
/// the venue names it, and a leg whose acknowledgement was lost has none at all.
/// `order_id` is therefore optional and is filled in as soon as anything reports
/// one — the answer to a retry, a fill event, or the live order list.
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
    /// The instant this leg began being tracked. The attribution window opens
    /// here, and a restart must not reopen it.
    pub created_at: Timestamp,
}

impl LegRecord {
    /// The leg as a `PendingOrder`, once the venue has named it.
    pub fn pending_order(&self) -> Option<PendingOrder> {
        let order_id = self.order_id.clone()?;
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
            self.created_at,
        ))
    }

    pub fn should_cancel(&self, now: Timestamp) -> bool {
        let time_until_expiry = self.expiry_time - now;
        time_until_expiry.as_seconds() < CANCEL_LEAD.as_seconds() || now >= self.expiry_time
    }
}

/// One corrective order the engine has decided to send, or has sent.
///
/// The record exists from the moment the engine commits to the order, before the
/// venue has said anything about it, so `order_id` is optional: a submission whose
/// acknowledgement was lost has a `client_ref` and no id. Such a record still
/// counts as working at its full size, because the venue may have created the
/// order and the engine cannot yet see it.
#[derive(Debug, Clone, PartialEq)]
pub struct NeutralizationRecord {
    pub strategy_id: StrategyId,
    pub venue: Venue,
    pub instrument_id: InstrumentId,
    pub client_ref: ClientRef,
    pub order_id: Option<OrderId>,
    pub venue_seq: Option<u64>,
    pub limit_price: Price,
    pub size: Qty,
    pub expiry_time: Timestamp,
    /// The instant the order began being tracked: the opening of the attribution
    /// window that opened at the first attempt, not at a later retry.
    pub window_start: Timestamp,
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
    /// Engine clock at the instant this order began being tracked. Preserved
    /// across restarts and across retries, so a restart cannot reopen a window
    /// and admit an observation from outside the original one.
    pub window_start: Timestamp,
    pub venue_seq: Option<u64>,
    pub size: Qty,
    /// Highest total ever reported for this order by a fill event or by the live
    /// order list. Both state the same cumulative figure, so this is a maximum
    /// and never a sum.
    pub direct_filled: Qty,
    /// Quantity attributed to this order by block matches. Additive, and made
    /// idempotent by `block_ref` rather than by a maximum.
    pub block_filled: Qty,
    /// Set once a block naming this order has been observed: the venue has closed
    /// it, and it will never acquire another base unit.
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
/// Nothing here is keyed by `order_id`. The venue recycles identifiers once a
/// window has closed, so durable state keyed by one speaks only for the order
/// that held it while its window was open; every set below is keyed by the
/// `client_ref` the engine chose instead.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub positions: Vec<Position>,
    pub legs: Vec<LegRecord>,
    pub attempted_strategies: Vec<StrategyId>,
    /// The client-reference counter. A reset counter regenerates a reference the
    /// venue already knows, and the venue deduplicates it onto an unrelated order.
    pub client_seq: u64,
    pub fill_cursor: Cursor,
    pub submissions: Vec<SubmissionRecord>,
    pub absorbed_blocks: Vec<BlockRef>,
    /// Legs marked for closure, by client reference.
    pub closing: Vec<ClientRef>,
    /// Legs this engine has cancelled at the venue, by client reference.
    pub replaced: Vec<ClientRef>,
    pub neutralizations: Vec<NeutralizationRecord>,
}
