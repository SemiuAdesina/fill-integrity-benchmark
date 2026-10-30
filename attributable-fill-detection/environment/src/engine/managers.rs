use std::collections::BTreeMap;

use crate::types::ids::{StrategyId, OrderId, Venue};
use crate::types::pending_order::PendingOrder;
use crate::types::position::Position;
use crate::types::time::Timestamp;

#[derive(Debug, Clone, Default)]
pub struct PendingOrderManager {
    orders: BTreeMap<OrderId, PendingOrder>,
}

impl PendingOrderManager {
    pub fn new() -> Self {
        Self {
            orders: BTreeMap::new(),
        }
    }

    pub fn from_orders(orders: Vec<PendingOrder>) -> Self {
        let mut manager = Self::new();
        for order in orders {
            manager.insert(order);
        }
        manager
    }

    pub fn insert(&mut self, order: PendingOrder) {
        self.orders.insert(order.order_id.clone(), order);
    }

    pub fn remove(&mut self, order_id: &OrderId) -> Option<PendingOrder> {
        self.orders.remove(order_id)
    }

    pub fn get(&self, order_id: &OrderId) -> Option<&PendingOrder> {
        self.orders.get(order_id)
    }

    pub fn leg(&self, strategy_id: &StrategyId, venue: Venue) -> Option<&PendingOrder> {
        self.orders
            .values()
            .find(|order| &order.strategy_id == strategy_id && order.venue == venue)
    }

    pub fn strategy_ids(&self) -> Vec<StrategyId> {
        let mut ids: Vec<StrategyId> = self
            .orders
            .values()
            .map(|order| order.strategy_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    pub fn expiring(&self, now: Timestamp) -> Vec<OrderId> {
        self.orders
            .values()
            .filter(|order| order.should_cancel(now))
            .map(|order| order.order_id.clone())
            .collect()
    }

    pub fn all(&self) -> Vec<PendingOrder> {
        self.orders.values().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.orders.len()
    }

    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct PositionManager {
    positions: BTreeMap<StrategyId, Position>,
}

impl PositionManager {
    pub fn new() -> Self {
        Self {
            positions: BTreeMap::new(),
        }
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
