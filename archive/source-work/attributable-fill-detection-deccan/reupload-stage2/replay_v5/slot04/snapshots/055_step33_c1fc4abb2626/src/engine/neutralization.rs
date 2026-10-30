//! Sizing and pricing the corrective order that closes a one-legged mandate.
//!
//! The caller decides how much is still unresolved; this only prices it and
//! declines when there is nothing left to cover.

use crate::engine::sizing::neutralization_limit_price;
use crate::ports::{OrderGateway, PortError};
use crate::types::ids::{InstrumentId, Price, Qty, StrategyId, Venue};

#[derive(Debug, Clone, PartialEq)]
pub struct NeutralizationPlan {
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    pub size: Qty,
    pub limit_price: Price,
}

/// Price a corrective order for `remaining_size` base units on the leg's venue.
///
/// Quantities are whole base units, so a residual of one unit is a real residual
/// and a residual of zero or less is not an exposure at all.
pub fn plan_neutralization(
    gateway: &dyn OrderGateway,
    strategy_id: StrategyId,
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
        strategy_id,
        instrument_id: instrument_id.clone(),
        venue,
        size: remaining_size,
        limit_price: neutralization_limit_price(best_offer, slippage),
    }))
}
