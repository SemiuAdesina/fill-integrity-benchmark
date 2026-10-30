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

/// Price a corrective order for `remaining_size` base units on one venue.
///
/// The caller decides how much is still unresolved; this function only prices it
/// and declines when there is nothing left to cover. Quantities are whole base
/// units, so a residual of one unit is a real residual and a residual of zero is
/// not an exposure.
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
