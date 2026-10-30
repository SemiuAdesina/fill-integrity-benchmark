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

/// What an order the engine submitted is for.
///
/// An `Entry` order is one half of a paired mandate, resting on its venue. A
/// `Neutralization` order is the corrective order the engine sends on the venue
/// that did not fill, to close the exposure a one-legged mandate left open.
/// Both are orders this engine submitted, so both carry an attribution record
/// and an attribution window; they differ only in what the engine does with
/// what they fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderRole {
    Entry,
    Neutralization,
}

/// One order this engine submitted, and the attribution state held against it.
///
/// `client_ref` is the engine's own key and is always known: it is the only key
/// the engine has before the venue has answered, and the only key that survives
/// a lost acknowledgement. `order_id` and `venue_seq` come from the venue and
/// are therefore optional — an order whose acknowledgement was lost has neither
/// until a later observation supplies them.
///
/// Everything durable about an order lives here rather than in a set keyed by
/// `order_id`, because the venue recycles identifiers once an attribution
/// window has closed: state keyed by an identifier speaks only for the order
/// that wore it while its window was open.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmissionRecord {
    pub client_ref: ClientRef,
    pub order_id: Option<OrderId>,
    /// The instant the engine began tracking this order. The attribution window
    /// is `[window_start, window_start + ATTRIBUTION_WINDOW)`; a retry under the
    /// same client reference does not reopen it.
    pub window_start: Timestamp,
    pub venue_seq: Option<u64>,
    pub size: Qty,
    /// Quantity attributed to this order by per-order fill events and by the
    /// live order list. Cumulative: the maximum ever observed, never a sum.
    pub direct_filled: Qty,
    /// Quantity attributed to this order by block matches. Additive: one
    /// increment per `block_ref`, absorbed at most once.
    pub block_filled: Qty,
    /// Set once a block naming this order has been observed. The venue has
    /// closed it, so it can acquire nothing more.
    pub finalized: bool,
    pub role: OrderRole,
    pub kind: OrderKind,
    pub strategy_id: StrategyId,
    pub venue: Venue,
    pub instrument_id: InstrumentId,
    pub limit_price: Price,
    pub expiry_time: Timestamp,
    /// The engine has moved this order into closure but has not yet had the
    /// cancellation accepted at the venue.
    pub closing: bool,
    /// The engine has cancelled this order at the venue.
    pub cancelled: bool,
    /// The engine has stopped working this order for good.
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

    /// Whether the venue has named this order, so that it can be resolved by
    /// identifier and cancelled.
    pub fn is_named(&self) -> bool {
        self.order_id.is_some()
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
