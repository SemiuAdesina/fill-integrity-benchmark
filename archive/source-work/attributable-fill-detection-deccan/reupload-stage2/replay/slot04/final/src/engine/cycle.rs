//! One engine cycle: read the venue, decide what completed, act, persist.
//!
//! `run_cycle` is the only entry point the graded suite calls. The order of the
//! steps below is the order they run in, and everything else in the crate is a
//! type or a helper reached from here.

use std::collections::BTreeSet;

use crate::engine::config::EngineConfig;
use crate::engine::fill_classifier::AttributionLedger;
use crate::engine::neutralization::plan_neutralization;
use crate::engine::managers::{PendingOrderManager, PositionManager};
use crate::engine::sizing::leg_size;
use crate::ports::{
    InventorySource, Clock, FillEventSource, StrategySource, OrderGateway, OrderKind, OrderRequest,
    PortError, StateStore, SubmitError,
};
use crate::types::ids::{ClientRef, StrategyId, OrderId, Qty, Venue};
use crate::types::strategy::StrategyCandidate;
use crate::types::pending_order::PendingOrder;
use crate::types::position::Position;
use crate::types::snapshot::{NeutralizationRecord, Snapshot};
use crate::types::time::Timestamp;

/// What the live order list currently shows about a leg the engine tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegVisibility {
    /// Present in `open_orders`.
    Resting,
    /// Absent from `open_orders`, and this engine cancelled it.
    Withdrawn,
    /// Absent from `open_orders`, and this engine did not cancel it.
    Unconfirmed,
}

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
    /// Per-order attribution state, and the feed cursor that produced it.
    ledger: AttributionLedger,
    /// Corrective orders this engine has committed to, resolved or not.
    neutralizations: Vec<NeutralizationRecord>,
    /// Legs marked for closure on an earlier cycle.
    closing: BTreeSet<OrderId>,
    /// Legs this engine has cancelled at the venue.
    replaced: BTreeSet<OrderId>,
    /// Order ids the last `open_orders` call reported as live. Not durable: it is
    /// re-read every cycle and says nothing after a restart.
    resting_at_venue: BTreeSet<OrderId>,
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
            ledger: AttributionLedger::new(),
            neutralizations: Vec::new(),
            closing: BTreeSet::new(),
            replaced: BTreeSet::new(),
            resting_at_venue: BTreeSet::new(),
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
        self.sync_observations();
        self.reconcile_neutralizations();
        self.enter_new_strategies(now)?;
        self.reconcile_open_strategies(now)?;
        self.retire_stale_orders(now);
        self.persist()?;
        Ok(())
    }

    /// Restore the durable state on the first cycle after a start or restart.
    fn ensure_loaded(&mut self) -> Result<(), PortError> {
        if self.loaded {
            return Ok(());
        }
        if let Some(snapshot) = self.store.load()? {
            self.positions = PositionManager::from_positions(snapshot.positions);
            self.pending = PendingOrderManager::from_orders(snapshot.pending_orders);
            self.attempted = snapshot.attempted_strategies.into_iter().collect();
            self.client_seq = snapshot.client_seq;
            self.ledger = AttributionLedger::restore(
                snapshot.submissions,
                snapshot.absorbed_blocks,
                snapshot.fill_cursor,
            );
            self.closing = snapshot.closing_orders.into_iter().collect();
            self.replaced = snapshot.replaced_orders.into_iter().collect();
            self.neutralizations = snapshot.neutralizations;
        }
        self.loaded = true;
        Ok(())
    }

    /// Read both venue sources into the ledger before anything is decided.
    ///
    /// A failed `open_orders` says nothing about the venue, so no order can be
    /// assumed live from it.
    fn sync_observations(&mut self) {
        match self.gateway.open_orders() {
            Ok(open) => {
                self.ledger.absorb_open_orders(&open);
                self.resting_at_venue = open.into_iter().map(|order| order.order_id).collect();
            }
            Err(_) => {
                self.resting_at_venue.clear();
            }
        }
        if let Ok(batch) = self.fills.poll(&self.ledger.cursor()) {
            self.observed_fill_events += self.ledger.absorb_batch(batch);
        }
    }

    /// Match up corrective orders whose fate was unknown when they were sent.
    ///
    /// A record without an `order_id` is one the venue never acknowledged. The
    /// venue may or may not have created it, so it stays a commitment until the
    /// venue either names it or the attempt is shown to have created nothing.
    fn reconcile_neutralizations(&mut self) {
        // Identity first. The `client_ref` is echoed back on fill events and on
        // the open-order list, so an observation can name an order this engine
        // never got an acknowledgement for.
        let recovered: Vec<(ClientRef, OrderId)> = self
            .neutralizations
            .iter()
            .filter(|record| record.order_id.is_none())
            .filter_map(|record| {
                self.ledger
                    .order_for(&record.client_ref)
                    .map(|order_id| (record.client_ref.clone(), order_id))
            })
            .collect();
        for (client_ref, order_id) in recovered {
            if let Some(slot) = self.slot_mut(&client_ref) {
                slot.order_id = Some(order_id);
            }
        }

        // What is still unnamed may exist at the venue. Retry it under the same
        // `client_ref`: that reference is the venue's idempotency key, so the
        // retry resolves onto the original order, while a freshly minted one
        // would ask for the same economic exposure a second time.
        let unconfirmed: Vec<NeutralizationRecord> = self
            .neutralizations
            .iter()
            .filter(|record| record.order_id.is_none())
            .cloned()
            .collect();
        for record in unconfirmed {
            let request = Self::neutralization_request(&record);
            match self.gateway.submit(&request) {
                Ok(accepted) => {
                    self.ledger.track(
                        record.client_ref.clone(),
                        Some(accepted.order_id.clone()),
                        Some(accepted.venue_seq),
                        record.size,
                        record.submitted_at,
                    );
                    if let Some(slot) = self.slot_mut(&record.client_ref) {
                        slot.order_id = Some(accepted.order_id);
                    }
                }
                // Still unknown. Keep the record and try again on a later cycle.
                Err(SubmitError::AcknowledgementLost) => {}
                // Nothing was created, so this commitment is void and a later
                // cycle recomputes the residual. A mandate is not latched shut
                // by one failed attempt.
                Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                    self.neutralizations
                        .retain(|slot| slot.client_ref != record.client_ref);
                }
            }
        }
    }

    fn slot_mut(&mut self, client_ref: &ClientRef) -> Option<&mut NeutralizationRecord> {
        self.neutralizations
            .iter_mut()
            .find(|slot| slot.client_ref == *client_ref)
    }

    fn neutralization_request(record: &NeutralizationRecord) -> OrderRequest {
        OrderRequest {
            strategy_id: record.strategy_id.clone(),
            instrument_id: record.instrument_id.clone(),
            venue: record.venue,
            kind: OrderKind::ImmediateOrCancel,
            limit_price: record.limit_price,
            size: record.size,
            client_ref: record.client_ref.clone(),
            expiry_time: record.expiry_time,
        }
    }

    fn enter_new_strategies(&mut self, now: Timestamp) -> Result<(), PortError> {
        let available_cash = self.inventory.settlement_balance()?;
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
            let notional = (candidate.north_price * north_size as f64)
                + (candidate.south_price * south_size as f64);
            if notional > available_cash {
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
            self.resting_at_venue.insert(accepted.order_id.clone());
            self.ledger.track(
                client_ref.clone(),
                Some(accepted.order_id.clone()),
                Some(accepted.venue_seq),
                size,
                now,
            );
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

    /// Quantity attributable to this leg: the resting order itself, plus every
    /// corrective order this engine sent for the same mandate and venue.
    fn acquired_for(&self, leg: &PendingOrder) -> Qty {
        let mut total = self.ledger.attributed_for(&leg.client_ref);
        for record in self.records_for(leg) {
            total += self.ledger.attributed_for(&record.client_ref);
        }
        total
    }

    fn records_for(&self, leg: &PendingOrder) -> Vec<&NeutralizationRecord> {
        self.neutralizations
            .iter()
            .filter(|record| record.strategy_id == leg.strategy_id && record.venue == leg.venue)
            .collect()
    }

    /// Quantity still working in corrective orders this engine has already
    /// committed to for this leg.
    ///
    /// This is the term that stops consecutive cycles from sending the same
    /// exposure twice: a commitment counts as working until it resolves.
    fn working_for(&self, leg: &PendingOrder) -> Qty {
        let mut total = 0;
        for record in self.records_for(leg) {
            total += match record.order_id {
                // The venue has named it, so it contributes whatever it has
                // still to acquire. One the venue has since closed contributes
                // nothing, whatever it managed to fill: the venue cancelled the
                // rest and the gap it leaves behind is real.
                Some(_) if self.ledger.has(&record.client_ref) => {
                    self.ledger.outstanding_for(&record.client_ref)
                }
                // The acknowledgement was lost, so the venue may have created
                // the order and the engine cannot see it. Its full size counts
                // as working rather than nothing at all.
                _ => record.size.max(0),
            };
        }
        total
    }

    /// The exposure still open on this leg: what the mandate asked for, less
    /// what is already attributed to it, less what is already working.
    fn residual_for(&self, leg: &PendingOrder) -> Qty {
        leg.size - self.acquired_for(leg) - self.working_for(leg)
    }

    /// Exact integer comparison. There is no tolerance: a leg short by one base
    /// unit is short.
    fn leg_is_complete(&self, leg: &PendingOrder) -> bool {
        self.acquired_for(leg) >= leg.size
    }

    fn visibility(&self, leg: &PendingOrder) -> LegVisibility {
        if self.resting_at_venue.contains(&leg.order_id) {
            LegVisibility::Resting
        } else if self.replaced.contains(&leg.order_id) {
            LegVisibility::Withdrawn
        } else {
            LegVisibility::Unconfirmed
        }
    }

    fn reconcile_open_strategies(&mut self, now: Timestamp) -> Result<(), PortError> {
        for strategy_id in self.pending.strategy_ids() {
            if self.positions.has(&strategy_id) {
                continue;
            }
            let north_leg = match self.pending.leg(&strategy_id, Venue::North) {
                Some(order) => order.clone(),
                None => continue,
            };
            let south_leg = match self.pending.leg(&strategy_id, Venue::South) {
                Some(order) => order.clone(),
                None => continue,
            };

            let north_complete = self.leg_is_complete(&north_leg);
            let south_complete = self.leg_is_complete(&south_leg);

            if north_complete && south_complete {
                self.open_position(&north_leg, &south_leg, now);
            } else if north_complete {
                self.close_gap(&south_leg, now)?;
            } else if south_complete {
                self.close_gap(&north_leg, now)?;
            }
        }
        Ok(())
    }

    /// Close the exposure left by a leg that did not complete.
    ///
    /// Closing a resting leg is a two-step move: the first cycle marks it, the
    /// next one cancels it and covers what is missing. That gives an in-flight
    /// fill a cycle to arrive before another economic order is sent, and it means
    /// the cancellation is always observed before the correction that follows it.
    fn close_gap(&mut self, leg: &PendingOrder, now: Timestamp) -> Result<(), PortError> {
        match self.visibility(leg) {
            LegVisibility::Resting => {
                if self.closing.insert(leg.order_id.clone()) {
                    return Ok(());
                }
                if self.gateway.cancel(&leg.order_id).is_err() {
                    return Ok(());
                }
                self.resting_at_venue.remove(&leg.order_id);
                self.replaced.insert(leg.order_id.clone());
            }
            // Withdrawn by this engine, so there is nothing left to cancel.
            LegVisibility::Withdrawn => {}
            // Neither resting nor withdrawn by this engine: it may still be
            // working, and quantity it has already acquired may simply not have
            // come through the feed yet. Neutralizing against it would buy what
            // the desk already owns and double the exposure. Wait.
            LegVisibility::Unconfirmed => return Ok(()),
        }

        // Only the residual is covered: not the leg's size, and not the part of
        // it a corrective order already sent is working on.
        let residual = self.residual_for(leg);
        let plan = match plan_neutralization(
            self.gateway,
            leg,
            residual,
            self.config.neutralization_slippage,
        )? {
            Some(plan) => plan,
            None => return Ok(()),
        };

        // The record is written before the order is sent, so a submission whose
        // outcome is unknown is still something this engine knows it committed to.
        let client_ref = self.next_client_ref();
        let record = NeutralizationRecord {
            strategy_id: plan.strategy_id,
            venue: plan.venue,
            instrument_id: plan.instrument_id,
            client_ref: client_ref.clone(),
            order_id: None,
            limit_price: plan.limit_price,
            size: plan.size,
            expiry_time: leg.expiry_time,
            submitted_at: now,
        };
        let size = record.size;
        let request = Self::neutralization_request(&record);
        self.neutralizations.push(record);

        match self.gateway.submit(&request) {
            Ok(accepted) => {
                self.ledger.track(
                    client_ref.clone(),
                    Some(accepted.order_id.clone()),
                    Some(accepted.venue_seq),
                    size,
                    now,
                );
                if let Some(slot) = self.slot_mut(&client_ref) {
                    slot.order_id = Some(accepted.order_id);
                }
            }
            // The venue may have created this order and its identifier is
            // unknown. It stays a commitment, contributing its full size to what
            // is already working, and is retried under the same client
            // reference on a later cycle.
            Err(SubmitError::AcknowledgementLost) => {}
            // Nothing was created, so there is nothing to track. The exposure
            // stays open and a later cycle recomputes the residual.
            Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                self.neutralizations
                    .retain(|slot| slot.client_ref != client_ref);
            }
        }

        Ok(())
    }

    fn open_position(&mut self, north_leg: &PendingOrder, south_leg: &PendingOrder, now: Timestamp) {
        let position = Position::new(
            north_leg.strategy_id.clone(),
            north_leg.size,
            south_leg.size,
            north_leg.limit_price,
            south_leg.limit_price,
            north_leg.expiry_time,
            now,
        );
        let strategy_id = position.strategy_id.clone();
        self.positions.insert(position);
        self.pending.remove(&north_leg.order_id);
        self.pending.remove(&south_leg.order_id);
        self.neutralizations
            .retain(|record| record.strategy_id != strategy_id);
    }

    fn retire_stale_orders(&mut self, now: Timestamp) {
        for order_id in self.pending.expiring(now) {
            let _ = self.gateway.cancel(&order_id);
            self.pending.remove(&order_id);
            self.closing.remove(&order_id);
            self.replaced.remove(&order_id);
        }
    }

    fn persist(&self) -> Result<(), PortError> {
        let snapshot = Snapshot {
            positions: self.positions.all(),
            pending_orders: self.pending.all(),
            attempted_strategies: self.attempted.iter().cloned().collect(),
            client_seq: self.client_seq,
            fill_cursor: self.ledger.cursor(),
            submissions: self.ledger.submissions(),
            absorbed_blocks: self.ledger.absorbed_blocks(),
            closing_orders: self.closing.iter().cloned().collect(),
            replaced_orders: self.replaced.iter().cloned().collect(),
            neutralizations: self.neutralizations.clone(),
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
