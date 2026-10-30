use std::collections::BTreeMap;

use crate::types::ids::{MarketId, OrderId, Side};
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

    pub fn leg(&self, condition_id: &MarketId, side: Side) -> Option<&PendingOrder> {
        self.orders
            .values()
            .find(|order| &order.condition_id == condition_id && order.side == side)
    }

    pub fn market_ids(&self) -> Vec<MarketId> {
        let mut ids: Vec<MarketId> = self
            .orders
            .values()
            .map(|order| order.condition_id.clone())
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
    positions: BTreeMap<MarketId, Position>,
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
            .insert(position.condition_id.clone(), position);
    }

    pub fn get(&self, condition_id: &MarketId) -> Option<&Position> {
        self.positions.get(condition_id)
    }

    pub fn has(&self, condition_id: &MarketId) -> bool {
        self.positions.contains_key(condition_id)
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
