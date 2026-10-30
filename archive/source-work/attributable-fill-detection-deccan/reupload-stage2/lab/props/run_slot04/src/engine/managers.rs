//! Positions the engine holds.
//!
//! Resting legs are not here: they are held by `Engine` keyed by the client
//! reference the engine chose, because the venue recycles order identifiers and a
//! container keyed by one would let a settled mandate's identifier speak for a
//! later order that comes to wear it.

use std::collections::BTreeMap;

use crate::types::ids::StrategyId;
use crate::types::position::Position;

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
