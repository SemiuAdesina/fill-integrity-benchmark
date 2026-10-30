//! The two collections the engine reasons over: entry legs still being worked,
//! and pairings already recorded.
//!
//! A leg is keyed by the `client_ref` the engine chose rather than by the venue's
//! `order_id`. The venue recycles identifiers once an attribution window has
//! closed, so anything the engine keeps keyed by an identifier — a leg, its
//! closure state, its cancellation — could otherwise come to speak for a later
//! order that wears the same label. A `client_ref` is minted from a persisted
//! counter and is never reused.

use std::collections::BTreeMap;

use crate::types::ids::{ClientRef, OrderId, StrategyId, Venue};
use crate::types::pending_order::{PendingOrder, CANCEL_LEAD};
use crate::types::position::Position;
use crate::types::snapshot::LegRecord;
use crate::types::time::Timestamp;

#[derive(Debug, Clone, Default)]
pub struct PendingOrderManager {
    legs: BTreeMap<ClientRef, LegRecord>,
}

impl PendingOrderManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_orders(legs: Vec<LegRecord>) -> Self {
        let mut manager = Self::new();
        for leg in legs {
            manager.insert(leg);
        }
        manager
    }

    pub fn insert(&mut self, leg: LegRecord) {
        self.legs.insert(leg.client_ref.clone(), leg);
    }

    pub fn remove(&mut self, client_ref: &ClientRef) -> Option<LegRecord> {
        self.legs.remove(client_ref)
    }

    pub fn get(&self, client_ref: &ClientRef) -> Option<&LegRecord> {
        self.legs.get(client_ref)
    }

    pub fn get_mut(&mut self, client_ref: &ClientRef) -> Option<&mut LegRecord> {
        self.legs.get_mut(client_ref)
    }

    /// The legs of one mandate, north first then south.
    pub fn for_strategy(&self, strategy_id: &StrategyId) -> Vec<&LegRecord> {
        let mut legs: Vec<&LegRecord> = self
            .legs
            .values()
            .filter(|leg| &leg.strategy_id == strategy_id)
            .collect();
        legs.sort_by_key(|leg| leg.venue);
        legs
    }

    pub fn leg(&self, strategy_id: &StrategyId, venue: Venue) -> Option<&LegRecord> {
        self.legs
            .values()
            .find(|leg| &leg.strategy_id == strategy_id && leg.venue == venue)
    }

    pub fn leg_mut(&mut self, strategy_id: &StrategyId, venue: Venue) -> Option<&mut LegRecord> {
        self.legs
            .values_mut()
            .find(|leg| &leg.strategy_id == strategy_id && leg.venue == venue)
    }

    pub fn contains(&self, strategy_id: &StrategyId, venue: Venue) -> bool {
        self.leg(strategy_id, venue).is_some()
    }

    pub fn strategy_ids(&self) -> Vec<StrategyId> {
        let mut ids: Vec<StrategyId> = self
            .legs
            .values()
            .map(|leg| leg.strategy_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// Legs whose expiry is close enough that they have to come off the book.
    pub fn expiring(&self, now: Timestamp) -> Vec<ClientRef> {
        self.legs
            .values()
            .filter(|leg| {
                (leg.expiry_time - now).as_seconds() < CANCEL_LEAD.as_seconds()
                    || now >= leg.expiry_time
            })
            .map(|leg| leg.client_ref.clone())
            .collect()
    }

    /// Legs past their expiry, so no further attempt is worth making for them.
    pub fn expired(&self, now: Timestamp) -> Vec<ClientRef> {
        self.legs
            .values()
            .filter(|leg| now >= leg.expiry_time)
            .map(|leg| leg.client_ref.clone())
            .collect()
    }

    pub fn leg_refs(&self) -> Vec<ClientRef> {
        self.legs.keys().cloned().collect()
    }

    /// Every leg, in a stable order.
    pub fn all_legs(&self) -> Vec<LegRecord> {
        self.legs.values().cloned().collect()
    }

    /// The legs the engine reports as outstanding. A leg the venue has not yet
    /// named — one whose acknowledgement was lost — carries no venue identifier,
    /// so it is not yet an order the world can address.
    pub fn all(&self) -> Vec<PendingOrder> {
        self.legs
            .values()
            .filter(|leg| leg.order_id.is_some())
            .map(Self::to_pending)
            .collect()
    }

    fn to_pending(leg: &LegRecord) -> PendingOrder {
        PendingOrder::new(
            leg.strategy_id.clone(),
            leg.instrument_id.clone(),
            leg.venue,
            leg.order_id
                .clone()
                .unwrap_or_else(|| OrderId::new(leg.client_ref.as_str())),
            leg.client_ref.clone(),
            leg.venue_seq.unwrap_or(0),
            leg.limit_price,
            leg.size,
            leg.expiry_time,
            leg.created_at,
        )
    }

    pub fn len(&self) -> usize {
        self.legs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.legs.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct PositionManager {
    positions: BTreeMap<StrategyId, Position>,
}

impl PositionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_positions(positions: Vec<Position>) -> Self {
        let mut manager = Self::new();
        for position in positions {
            manager.insert(position);
        }
        manager
    }

    pub fn insert(&mut self, position: Position) {
        self.positions
            .insert(position.strategy_id.clone(), position);
    }

    pub fn get(&self, strategy_id: &StrategyId) -> Option<&Position> {
        self.positions.get(strategy_id)
    }

    pub fn has(&self, strategy_id: &StrategyId) -> bool {
        self.positions.contains_key(strategy_id)
    }

    pub fn all(&self) -> Vec<Position> {
        self.positions.values().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}
