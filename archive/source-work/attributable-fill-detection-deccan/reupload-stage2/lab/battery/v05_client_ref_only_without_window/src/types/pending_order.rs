use crate::types::ids::{ClientRef, InstrumentId, OrderId, Price, Qty, StrategyId, Venue};
use crate::types::time::{Duration, Timestamp};

pub const CANCEL_LEAD: Duration = Duration::from_seconds(300);

/// How long an order's attribution window stays open.
///
/// The window opens at the instant the engine begins tracking the order and
/// runs for this duration. See `PendingOrder` for the half-open rule and
/// `/app/spec/venue-semantics.md` for why the venue guarantees it.
pub const ATTRIBUTION_WINDOW: Duration = Duration::from_seconds(900);

#[derive(Debug, Clone, PartialEq)]
pub struct PendingOrder {
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    pub order_id: OrderId,
    pub client_ref: ClientRef,
    /// The sequence number the venue assigned when it accepted this order.
    /// Orders accepted in the same venue tick share one sequence number, so it
    /// orders arrivals but does not uniquely identify an order.
    pub venue_seq: u64,
    pub limit_price: Price,
    pub size: Qty,
    pub expiry_time: Timestamp,
    pub created_at: Timestamp,
}

impl PendingOrder {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        venue: Venue,
        order_id: OrderId,
        client_ref: ClientRef,
        venue_seq: u64,
        limit_price: Price,
        size: Qty,
        expiry_time: Timestamp,
        created_at: Timestamp,
    ) -> Self {
        Self {
            strategy_id,
            instrument_id,
            venue,
            order_id,
            client_ref,
            venue_seq,
            limit_price,
            size,
            expiry_time,
            created_at,
        }
    }

    /// First instant at which an observation can be attributed to this order.
    pub fn attribution_start(&self) -> Timestamp {
        self.created_at
    }

    /// First instant at which an observation can no longer be attributed to
    /// this order. The window is half-open, so this instant is *excluded*.
    pub fn attribution_end(&self) -> Timestamp {
        self.created_at + ATTRIBUTION_WINDOW
    }

    pub fn should_cancel(&self, now: Timestamp) -> bool {
        let time_until_expiry = self.expiry_time - now;
        time_until_expiry.as_seconds() < CANCEL_LEAD.as_seconds() || now >= self.expiry_time
    }

    pub fn seconds_until_expiry(&self, now: Timestamp) -> i64 {
        (self.expiry_time - now).as_seconds()
    }
}
