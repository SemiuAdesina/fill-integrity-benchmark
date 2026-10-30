//! Pricing a corrective order.
//!
//! The caller works out how much of a leg is still unresolved; this module only
//! prices that quantity and declines when it cannot.

use crate::engine::sizing::neutralization_limit_price;
use crate::ports::{OrderGateway, PortError};
use crate::types::ids::{StrategyId, Price, Qty, Venue, InstrumentId};

#[derive(Debug, Clone, PartialEq)]
pub struct NeutralizationPlan {
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    pub size: Qty,
    pub limit_price: Price,
}

/// Price a corrective order for `remaining_size` base units on a leg's venue.
///
/// Quantities are whole base units, so a residual of one unit is a real residual
/// and a residual of zero or less is not an exposure. Without a quote to price
/// against there is nothing defensible to send, so the plan is declined and a
/// later cycle recomputes the residual and tries again.
pub fn plan_neutralization(
    gateway: &dyn OrderGateway,
    strategy_id: &StrategyId,
    instrument_id: &InstrumentId,
    venue: Venue,
    remaining_size: Qty,
    slippage: Price,
) -> Result<Option<NeutralizationPlan>, PortError> {
    if remaining_size <= 0 {
        return Ok(None);
    }
    let best_offer = match gateway.best_offer(instrument_id)? {
        Some(price) => price,
        None => return Ok(None),
    };
    Ok(Some(NeutralizationPlan {
        strategy_id: strategy_id.clone(),
        instrument_id: instrument_id.clone(),
        venue,
        size: remaining_size,
        limit_price: neutralization_limit_price(best_offer, slippage),
    }))
}
