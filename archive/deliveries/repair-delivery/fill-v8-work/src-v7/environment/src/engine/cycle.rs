use std::collections::BTreeSet;

use crate::engine::config::EngineConfig;
use crate::engine::fill_classifier::classify_legs;
use crate::engine::neutralization::plan_neutralization;
use crate::engine::managers::{PendingOrderManager, PositionManager};
use crate::engine::sizing::leg_size;
use crate::ports::{
    InventorySource, Clock, FillEventSource, StrategySource, OrderGateway, OrderKind, OrderRequest,
    PortError, StateStore, SubmitError,
};
use crate::types::ids::{ClientRef, StrategyId, Qty, Venue};
use crate::types::strategy::StrategyCandidate;
use crate::types::pending_order::PendingOrder;
use crate::types::position::Position;
use crate::types::snapshot::{Cursor, Snapshot};
use crate::types::time::Timestamp;

pub struct Engine<'a> {
    config: EngineConfig,
    strategies: &'a dyn StrategySource,
    inventory: &'a dyn InventorySource,
    gateway: &'a dyn OrderGateway,
    fills: &'a dyn FillEventSource,
    store: &'a dyn StateStore,
    clock: &'a dyn Clock,
    pending: PendingOrderManager,
    positions: PositionManager,
    attempted: BTreeSet<StrategyId>,
    loaded: bool,
    client_seq: u64,
    observed_fill_events: usize,
}

impl<'a> Engine<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: EngineConfig,
        strategies: &'a dyn StrategySource,
        inventory: &'a dyn InventorySource,
        gateway: &'a dyn OrderGateway,
        fills: &'a dyn FillEventSource,
        store: &'a dyn StateStore,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            config,
            strategies,
            inventory,
            gateway,
            fills,
            store,
            clock,
            pending: PendingOrderManager::new(),
            positions: PositionManager::new(),
            attempted: BTreeSet::new(),
            loaded: false,
            client_seq: 0,
            observed_fill_events: 0,
        }
    }

    pub fn positions(&self) -> Vec<Position> {
        self.positions.all()
    }

    pub fn pending_orders(&self) -> Vec<PendingOrder> {
        self.pending.all()
    }

    pub fn observed_fill_events(&self) -> usize {
        self.observed_fill_events
    }

    pub fn run_cycle(&mut self) -> Result<(), PortError> {
        self.ensure_loaded()?;
        let now = self.clock.now();
        self.refresh_fill_stream();
        self.enter_new_strategies(now)?;
        self.reconcile_open_strategies(now)?;
        self.retire_stale_orders(now);
        self.persist()?;
        Ok(())
    }

    fn ensure_loaded(&mut self) -> Result<(), PortError> {
        if self.loaded {
            return Ok(());
        }
        if let Some(snapshot) = self.store.load()? {
            self.positions = PositionManager::from_positions(snapshot.positions);
            self.pending = PendingOrderManager::from_orders(snapshot.pending_orders);
            self.attempted = snapshot.attempted_strategies.into_iter().collect();
        }
        self.loaded = true;
        Ok(())
    }

    fn refresh_fill_stream(&mut self) {
        if let Ok(batch) = self.fills.poll(&Cursor::beginning()) {
            self.observed_fill_events = batch.events.len();
        }
    }

    fn enter_new_strategies(&mut self, now: Timestamp) -> Result<(), PortError> {
        let candidates = self.strategies.candidates()?;
        for candidate in candidates {
            if self.positions.has(&candidate.strategy_id) {
                continue;
            }
            if self.attempted.contains(&candidate.strategy_id) {
                continue;
            }
            if candidate.pair_cost() > self.config.max_pair_cost {
                continue;
            }
            if now >= candidate.expiry_time {
                continue;
            }
            let north_size = leg_size(candidate.north_depth, self.config.max_leg_size);
            let south_size = leg_size(candidate.south_depth, self.config.max_leg_size);
            if north_size < self.config.min_leg_size || south_size < self.config.min_leg_size {
                continue;
            }
            self.attempted.insert(candidate.strategy_id.clone());
            self.place_leg(&candidate, Venue::North, north_size, now);
            self.place_leg(&candidate, Venue::South, south_size, now);
        }
        Ok(())
    }

    fn place_leg(&mut self, candidate: &StrategyCandidate, venue: Venue, size: Qty, now: Timestamp) {
        let client_ref = self.next_client_ref();
        let request = OrderRequest {
            strategy_id: candidate.strategy_id.clone(),
            instrument_id: candidate.instrument_for(venue).clone(),
            venue,
            kind: OrderKind::RestingLimit,
            limit_price: candidate.price_for(venue),
            size,
            client_ref: client_ref.clone(),
            expiry_time: candidate.expiry_time,
        };
        if let Ok(accepted) = self.gateway.submit(&request) {
            self.pending.insert(PendingOrder::new(
                candidate.strategy_id.clone(),
                candidate.instrument_for(venue).clone(),
                venue,
                accepted.order_id,
                client_ref,
                accepted.venue_seq,
                request.limit_price,
                size,
                candidate.expiry_time,
                now,
            ));
        }
    }

    fn reconcile_open_strategies(&mut self, now: Timestamp) -> Result<(), PortError> {
        for strategy_id in self.pending.strategy_ids() {
            if self.positions.has(&strategy_id) {
                continue;
            }
            let north_order = match self.pending.leg(&strategy_id, Venue::North) {
                Some(order) => order.clone(),
                None => continue,
            };
            let south_order = match self.pending.leg(&strategy_id, Venue::South) {
                Some(order) => order.clone(),
                None => continue,
            };

            let status = match classify_legs(self.inventory, &north_order, &south_order) {
                Ok(status) => status,
                Err(_) => continue,
            };

            if status.both_filled() {
                self.open_position(&north_order, &south_order, now);
            } else if status.north_filled {
                self.neutralize_missing_leg(&north_order, &south_order, now)?;
            } else if status.south_filled {
                self.neutralize_missing_leg(&south_order, &north_order, now)?;
            }
        }
        Ok(())
    }

    fn neutralize_missing_leg(
        &mut self,
        filled_leg: &PendingOrder,
        missing_leg: &PendingOrder,
        now: Timestamp,
    ) -> Result<(), PortError> {
        let plan = match plan_neutralization(self.gateway, missing_leg, self.config.neutralization_slippage)? {
            Some(plan) => plan,
            None => return Ok(()),
        };

        let _ = self.gateway.cancel(&missing_leg.order_id);

        let client_ref = self.next_client_ref();
        let request = OrderRequest {
            strategy_id: plan.strategy_id.clone(),
            instrument_id: plan.instrument_id.clone(),
            venue: plan.venue,
            kind: OrderKind::ImmediateOrCancel,
            limit_price: plan.limit_price,
            size: plan.size,
            client_ref,
            expiry_time: missing_leg.expiry_time,
        };

        match self.gateway.submit(&request) {
            Ok(_) => {
                if filled_leg.venue == Venue::North {
                    self.open_position(filled_leg, missing_leg, now);
                } else {
                    self.open_position(missing_leg, filled_leg, now);
                }
            }
            Err(SubmitError::AcknowledgementLost) => {}
            Err(_) => {}
        }

        Ok(())
    }

    fn open_position(&mut self, north_order: &PendingOrder, south_order: &PendingOrder, now: Timestamp) {
        let position = Position::new(
            north_order.strategy_id.clone(),
            north_order.size,
            south_order.size,
            north_order.limit_price,
            south_order.limit_price,
            north_order.expiry_time,
            now,
        );
        self.positions.insert(position);
        self.pending.remove(&north_order.order_id);
        self.pending.remove(&south_order.order_id);
    }

    fn retire_stale_orders(&mut self, now: Timestamp) {
        for order_id in self.pending.expiring(now) {
            let _ = self.gateway.cancel(&order_id);
            self.pending.remove(&order_id);
        }
    }

    fn persist(&self) -> Result<(), PortError> {
        let snapshot = Snapshot {
            positions: self.positions.all(),
            pending_orders: self.pending.all(),
            attempted_strategies: self.attempted.iter().cloned().collect(),
        };
        self.store.save(&snapshot)
    }

    fn next_client_ref(&mut self) -> ClientRef {
        self.client_seq += 1;
        ClientRef::new(format!(
            "{}-{}",
            self.config.client_ref_prefix, self.client_seq
        ))
    }
}
