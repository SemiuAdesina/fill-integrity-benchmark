//! One engine cycle: read the venue, decide what completed, act, persist.
//!
//! `run_cycle` is the only entry point the graded suite calls. The order of the
//! steps below is the order they run in, and everything else in the crate is a
//! type or a helper reached from here.

use std::collections::BTreeSet;

use crate::engine::config::EngineConfig;
use crate::engine::fill_classifier::AttributionLedger;
use crate::engine::neutralization::plan_neutralization;
use crate::engine::managers::PositionManager;
use crate::engine::sizing::leg_size;
use crate::ports::{
    InventorySource, Clock, FillEventSource, StrategySource, OrderGateway, OrderKind, OrderRequest,
    PortError, StateStore, SubmitError,
};
use crate::types::ids::{ClientRef, StrategyId, Qty, Venue};
use crate::types::strategy::StrategyCandidate;
use crate::types::pending_order::{PendingOrder, CANCEL_LEAD};
use crate::types::position::Position;
use crate::types::snapshot::{LegRecord, NeutralizationRecord, Snapshot, SubmissionState};
use crate::types::time::Timestamp;

/// What the venue and this engine's own history say about a leg right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegVisibility {
    /// Live at the venue, and resting there.
    Resting,
    /// Not live, and known to be gone: this engine cancelled it, or the venue
    /// closed it by naming it in a block. Nothing about it is in flight.
    Withdrawn,
    /// Not live, and not known to be gone: it may have filled with the fill not
    /// yet through the feed. Quantity the desk already owns must not be
    /// corrected against.
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
    positions: PositionManager,
    /// Every leg still being worked, keyed by the client reference this engine
    /// chose for it. Durable: identity, closure state and the reference a retry
    /// depends on all live here.
    legs: Vec<LegRecord>,
    attempted: BTreeSet<StrategyId>,
    /// Per-order attribution state, and the feed cursor that produced it.
    ledger: AttributionLedger,
    /// Corrective orders this engine has committed to, resolved or not.
    neutralizations: Vec<NeutralizationRecord>,
    /// Client references the last `open_orders` call reported as live. Not
    /// durable: it is re-read every cycle and says nothing after a restart.
    resting_at_venue: BTreeSet<ClientRef>,
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
            positions: PositionManager::new(),
            legs: Vec::new(),
            attempted: BTreeSet::new(),
            ledger: AttributionLedger::new(),
            neutralizations: Vec::new(),
            resting_at_venue: BTreeSet::new(),
            loaded: false,
            client_seq: 0,
            observed_fill_events: 0,
        }
    }

    pub fn positions(&self) -> Vec<Position> {
        self.positions.all()
    }

    /// The legs still outstanding at the venue.
    ///
    /// A leg the venue never named is not an order anyone can point at, and a
    /// leg this engine cancelled or the venue closed in a block is no longer
    /// outstanding, so neither is reported here.
    pub fn pending_orders(&self) -> Vec<PendingOrder> {
        self.legs
            .iter()
            .filter(|leg| !leg.cancelled && !self.ledger.is_finalized(&leg.client_ref))
            .filter_map(|leg| {
                let order_id = leg.order_id.clone()?;
                Some(PendingOrder::new(
                    leg.strategy_id.clone(),
                    leg.instrument_id.clone(),
                    leg.venue,
                    order_id,
                    leg.client_ref.clone(),
                    leg.venue_seq.unwrap_or(0),
                    leg.limit_price,
                    leg.size,
                    leg.expiry_time,
                    leg.created_at,
                ))
            })
            .collect()
    }

    pub fn observed_fill_events(&self) -> usize {
        self.observed_fill_events
    }

    pub fn run_cycle(&mut self) -> Result<(), PortError> {
        self.ensure_loaded()?;
        let now = self.clock.now();
        self.sync_observations();
        self.adopt_identities();
        self.reconcile_submissions(now);
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
            self.legs = snapshot.legs;
            self.attempted = snapshot.attempted_strategies.into_iter().collect();
            self.client_seq = snapshot.client_seq;
            self.ledger = AttributionLedger::restore(
                snapshot.submissions,
                snapshot.absorbed_blocks,
                snapshot.fill_cursor,
            );
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
                self.resting_at_venue = open.into_iter().map(|order| order.client_ref).collect();
            }
            Err(_) => {
                self.resting_at_venue.clear();
            }
        }
        if let Ok(batch) = self.fills.poll(&self.ledger.cursor()) {
            self.observed_fill_events += self.ledger.absorb_batch(batch);
        }
    }

    /// An order whose acknowledgement was lost has an identity again as soon as
    /// any observation carries its client reference.
    fn adopt_identities(&mut self) {
        for index in 0..self.legs.len() {
            if self.legs[index].order_id.is_some() {
                continue;
            }
            if let Some(order_id) = self.ledger.order_for(&self.legs[index].client_ref) {
                self.legs[index].order_id = Some(order_id);
                self.legs[index].state = SubmissionState::Acknowledged;
            }
        }
        for index in 0..self.neutralizations.len() {
            if self.neutralizations[index].order_id.is_some() {
                continue;
            }
            if let Some(order_id) = self.ledger.order_for(&self.neutralizations[index].client_ref) {
                self.neutralizations[index].order_id = Some(order_id);
                self.neutralizations[index].state = SubmissionState::Acknowledged;
            }
        }
    }

    /// Retry every submission the venue has not confirmed, under the client
    /// reference it was first sent under.
    ///
    /// The reference is the venue's idempotency key, so a retry lands on the
    /// original order rather than placing the same economic exposure twice. A
    /// freshly minted reference would place it again.
    fn reconcile_submissions(&mut self, now: Timestamp) {
        let unresolved: Vec<LegRecord> = self
            .legs
            .iter()
            .filter(|leg| leg.order_id.is_none())
            .cloned()
            .collect();
        for leg in unresolved {
            if leg.state == SubmissionState::Unsent {
                // No order exists at the venue, so no window has opened for it
                // yet. Reopening one that never opened is the difference between
                // attributing this attempt's fills and dropping them.
                self.ledger.reopen_window(&leg.client_ref, now);
            }
            let request = Self::leg_request(&leg);
            let size = leg.size;
            let client_ref = leg.client_ref.clone();
            match self.gateway.submit(&request) {
                Ok(accepted) => {
                    self.ledger.track(
                        client_ref.clone(),
                        Some(accepted.order_id.clone()),
                        Some(accepted.venue_seq),
                        size,
                        now,
                    );
                    self.accept_leg(&client_ref, accepted.order_id, accepted.venue_seq);
                    self.resting_at_venue.insert(client_ref);
                }
                Err(SubmitError::AcknowledgementLost) => {
                    self.set_leg_state(&client_ref, SubmissionState::Unacknowledged);
                }
                // Nothing was created, so the exposure is still open and a later
                // cycle tries again. A mandate is not latched shut by one failed
                // attempt.
                Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                    self.set_leg_state(&client_ref, SubmissionState::Unsent);
                }
            }
        }

        let unresolved: Vec<NeutralizationRecord> = self
            .neutralizations
            .iter()
            .filter(|record| record.order_id.is_none())
            .cloned()
            .collect();
        for record in unresolved {
            let request = Self::neutralization_request(&record);
            let size = record.size;
            let client_ref = record.client_ref.clone();
            match self.gateway.submit(&request) {
                Ok(accepted) => {
                    self.ledger.track(
                        client_ref.clone(),
                        Some(accepted.order_id.clone()),
                        Some(accepted.venue_seq),
                        size,
                        now,
                    );
                    if let Some(slot) = self
                        .neutralizations
                        .iter_mut()
                        .find(|slot| slot.client_ref == client_ref)
                    {
                        slot.order_id = Some(accepted.order_id);
                        slot.state = SubmissionState::Acknowledged;
                    }
                }
                Err(SubmitError::AcknowledgementLost) => {
                    if let Some(slot) = self
                        .neutralizations
                        .iter_mut()
                        .find(|slot| slot.client_ref == client_ref)
                    {
                        slot.state = SubmissionState::Unacknowledged;
                    }
                }
                // No economic order exists, so the exposure this attempt was
                // meant to cover is open again and a later cycle recomputes it.
                Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                    self.ledger.forget(&client_ref);
                    self.neutralizations
                        .retain(|slot| slot.client_ref != client_ref);
                }
            }
        }
    }

    fn leg_request(leg: &LegRecord) -> OrderRequest {
        OrderRequest {
            strategy_id: leg.strategy_id.clone(),
            instrument_id: leg.instrument_id.clone(),
            venue: leg.venue,
            kind: OrderKind::RestingLimit,
            limit_price: leg.limit_price,
            size: leg.size,
            client_ref: leg.client_ref.clone(),
            expiry_time: leg.expiry_time,
        }
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

    fn accept_leg(&mut self, client_ref: &ClientRef, order_id: crate::types::ids::OrderId, venue_seq: u64) {
        if let Some(leg) = self.legs.iter_mut().find(|leg| leg.client_ref == *client_ref) {
            leg.order_id = Some(order_id);
            leg.venue_seq = Some(venue_seq);
            leg.state = SubmissionState::Acknowledged;
        }
    }

    fn set_leg_state(&mut self, client_ref: &ClientRef, state: SubmissionState) {
        if let Some(leg) = self.legs.iter_mut().find(|leg| leg.client_ref == *client_ref) {
            leg.state = state;
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
            if self.legs.iter().any(|leg| leg.strategy_id == candidate.strategy_id) {
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

    /// Rest one limit order for one leg of a mandate.
    ///
    /// The leg is tracked from this first attempt, before the venue answers: an
    /// order whose acknowledgement is lost is one the venue may have created, and
    /// its attribution window is the one that opened here.
    fn place_leg(&mut self, candidate: &StrategyCandidate, venue: Venue, size: Qty, now: Timestamp) {
        let client_ref = self.next_client_ref();
        let leg = LegRecord {
            strategy_id: candidate.strategy_id.clone(),
            instrument_id: candidate.instrument_for(venue).clone(),
            venue,
            client_ref: client_ref.clone(),
            order_id: None,
            venue_seq: None,
            limit_price: candidate.price_for(venue),
            size,
            expiry_time: candidate.expiry_time,
            created_at: now,
            state: SubmissionState::Unsent,
            closing: false,
            cancelled: false,
        };
        let request = Self::leg_request(&leg);
        self.legs.push(leg);
        self.ledger.track(client_ref.clone(), None, None, size, now);

        match self.gateway.submit(&request) {
            Ok(accepted) => {
                self.ledger.track(
                    client_ref.clone(),
                    Some(accepted.order_id.clone()),
                    Some(accepted.venue_seq),
                    size,
                    now,
                );
                self.accept_leg(&client_ref, accepted.order_id, accepted.venue_seq);
                self.resting_at_venue.insert(client_ref);
            }
            // The venue may hold the order and the engine cannot name it. The
            // client reference is kept, and the retry lands on the same order.
            Err(SubmitError::AcknowledgementLost) => {
                self.set_leg_state(&client_ref, SubmissionState::Unacknowledged);
            }
            // Nothing was created. The leg stays tracked and a later cycle
            // retries it, rather than the mandate being latched shut.
            Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                self.set_leg_state(&client_ref, SubmissionState::Unsent);
            }
        }
    }

    /// Quantity attributable to this leg: the resting order itself, plus every
    /// corrective order this engine has already sent for the same mandate and
    /// venue. A corrective fill resolves the exposure, so it is not counted a
    /// second time as a fill of the resting order.
    fn acquired_for(&self, leg: &LegRecord) -> Qty {
        let mut total = self.ledger.attributed_for(&leg.client_ref);
        for record in &self.neutralizations {
            if record.strategy_id == leg.strategy_id && record.venue == leg.venue {
                total += self.ledger.attributed_for(&record.client_ref);
            }
        }
        total
    }

    /// What this leg still needs, after everything already attributed to it and
    /// everything already working for it.
    ///
    /// The second term is the fill of the leg's own resting order and of the
    /// corrective orders already sent for it. The third is what stops
    /// consecutive cycles from sending the same exposure twice.
    fn residual_for(&self, leg: &LegRecord) -> Qty {
        let mut attributed = self.ledger.attributed_for(&leg.client_ref);
        let mut working: Qty = 0;
        for record in &self.neutralizations {
            if record.strategy_id != leg.strategy_id || record.venue != leg.venue {
                continue;
            }
            let filled = self.ledger.attributed_for(&record.client_ref);
            attributed += filled;
            if self.ledger.is_finalized(&record.client_ref) {
                // The venue closed it. Whatever it managed to fill, the gap it
                // leaves behind is real, so it counts for nothing here.
                continue;
            }
            working += match record.order_id {
                // Acknowledged: still working for what it has not acquired.
                Some(_) => {
                    let left = record.size - filled;
                    if left > 0 {
                        left
                    } else {
                        0
                    }
                }
                // Unacknowledged: the venue may have created it and this engine
                // cannot see it, so the whole size counts as committed.
                None => {
                    if record.size > 0 {
                        record.size
                    } else {
                        0
                    }
                }
            };
        }
        let residual = leg.size - attributed - working;
        if residual > 0 {
            residual
        } else {
            0
        }
    }

    /// Exact integer comparison. There is no tolerance: a leg short by one base
    /// unit is short, and a leg over by one is met.
    fn leg_is_complete(&self, leg: &LegRecord) -> bool {
        self.acquired_for(leg) >= leg.size
    }

    fn visibility(&self, leg: &LegRecord) -> LegVisibility {
        if self.resting_at_venue.contains(&leg.client_ref) {
            LegVisibility::Resting
        } else if leg.cancelled || self.ledger.is_finalized(&leg.client_ref) {
            // Either this engine withdrew it, or the venue closed it by naming it
            // in a block. A block is the venue's own statement that the order is
            // closed, so nothing about it is in flight.
            LegVisibility::Withdrawn
        } else {
            LegVisibility::Unconfirmed
        }
    }

    fn leg_index(&self, strategy_id: &StrategyId, venue: Venue) -> Option<usize> {
        self.legs
            .iter()
            .position(|leg| &leg.strategy_id == strategy_id && leg.venue == venue)
    }

    fn tracked_strategies(&self) -> Vec<StrategyId> {
        let mut ids: Vec<StrategyId> = self
            .legs
            .iter()
            .map(|leg| leg.strategy_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    fn reconcile_open_strategies(&mut self, now: Timestamp) -> Result<(), PortError> {
        for strategy_id in self.tracked_strategies() {
            if self.positions.has(&strategy_id) {
                continue;
            }
            let north = match self.leg_index(&strategy_id, Venue::North) {
                Some(index) => index,
                None => continue,
            };
            let south = match self.leg_index(&strategy_id, Venue::South) {
                Some(index) => index,
                None => continue,
            };

            let north_complete = self.leg_is_complete(&self.legs[north].clone());
            let south_complete = self.leg_is_complete(&self.legs[south].clone());

            if north_complete && south_complete {
                self.open_position(north, south, now);
            } else if north_complete {
                self.close_gap(south, now)?;
            } else if south_complete {
                self.close_gap(north, now)?;
            }
        }
        Ok(())
    }

    /// Close the exposure left by a leg that did not complete.
    ///
    /// Closing a resting leg is a two-step move: the first cycle marks it, the
    /// next one cancels it and covers what is missing. That gives an in-flight
    /// fill a cycle in which to land and be attributed before another economic
    /// order is sent.
    fn close_gap(&mut self, index: usize, now: Timestamp) -> Result<(), PortError> {
        let leg = self.legs[index].clone();
        match self.visibility(&leg) {
            LegVisibility::Resting => {
                if !leg.closing {
                    if let Some(slot) = self.legs.get_mut(index) {
                        slot.closing = true;
                    }
                    return Ok(());
                }
                let Some(order_id) = leg.order_id.clone() else {
                    return Ok(());
                };
                if self.gateway.cancel(&order_id).is_err() {
                    return Ok(());
                }
                if let Some(slot) = self.legs.get_mut(index) {
                    slot.cancelled = true;
                }
                self.resting_at_venue.remove(&leg.client_ref);
            }
            // The venue closed it, or this engine did: nothing is in flight and
            // the residual is covered directly.
            LegVisibility::Withdrawn => {}
            // Not live and not known to be gone. Correcting quantity the desk
            // already owns doubles the exposure the correction was meant to
            // close, so this leg is left alone until something explains it.
            LegVisibility::Unconfirmed => return Ok(()),
        }

        let residual = self.residual_for(&leg);
        let plan = match plan_neutralization(
            self.gateway,
            &leg.strategy_id,
            &leg.instrument_id,
            leg.venue,
            residual,
            self.config.neutralization_slippage,
        )? {
            Some(plan) => plan,
            None => return Ok(()),
        };

        // The record is written before the order is sent, so a submission whose
        // outcome is unknown is still something this engine knows it committed
        // to. Its window opens at this first attempt.
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
            state: SubmissionState::Unacknowledged,
        };
        let size = record.size;
        let request = Self::neutralization_request(&record);
        self.ledger.track(client_ref.clone(), None, None, size, now);
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
                if let Some(slot) = self
                    .neutralizations
                    .iter_mut()
                    .find(|slot| slot.client_ref == client_ref)
                {
                    slot.order_id = Some(accepted.order_id);
                    slot.state = SubmissionState::Acknowledged;
                }
            }
            // The venue may hold it and this engine cannot name it, so the
            // record stands for the full size until something resolves it.
            Err(SubmitError::AcknowledgementLost) => {}
            // Nothing was created, so the exposure is still open and a later
            // cycle recomputes it against whatever has been attributed since.
            Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                self.ledger.forget(&client_ref);
                self.neutralizations
                    .retain(|slot| slot.client_ref != client_ref);
            }
        }

        Ok(())
    }

    fn open_position(&mut self, north: usize, south: usize, now: Timestamp) {
        let north_leg = self.legs[north].clone();
        let south_leg = self.legs[south].clone();
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
        // Settled: the venue is free to hand these identifiers to unrelated
        // orders, so nothing keyed by them may outlive the mandate.
        self.settle(&strategy_id);
    }

    fn settle(&mut self, strategy_id: &StrategyId) {
        let retired: Vec<ClientRef> = self
            .legs
            .iter()
            .filter(|leg| &leg.strategy_id == strategy_id)
            .map(|leg| leg.client_ref.clone())
            .chain(
                self.neutralizations
                    .iter()
                    .filter(|record| &record.strategy_id == strategy_id)
                    .map(|record| record.client_ref.clone()),
            )
            .collect();
        self.legs.retain(|leg| &leg.strategy_id != strategy_id);
        self.neutralizations
            .retain(|record| &record.strategy_id != strategy_id);
        for client_ref in retired {
            self.ledger.forget(&client_ref);
            self.resting_at_venue.remove(&client_ref);
        }
    }

    fn retiring(leg: &LegRecord, now: Timestamp) -> bool {
        let time_until_expiry = leg.expiry_time - now;
        time_until_expiry.as_seconds() < CANCEL_LEAD.as_seconds() || now >= leg.expiry_time
    }

    fn retire_stale_orders(&mut self, now: Timestamp) {
        let expiring: Vec<ClientRef> = self
            .legs
            .iter()
            .filter(|leg| Self::retiring(leg, now))
            .map(|leg| leg.client_ref.clone())
            .collect();
        for client_ref in expiring {
            let Some(leg) = self
                .legs
                .iter()
                .find(|leg| leg.client_ref == client_ref)
                .cloned()
            else {
                continue;
            };
            if let Some(order_id) = leg.order_id.clone() {
                let _ = self.gateway.cancel(&order_id);
            }
            self.legs.retain(|leg| leg.client_ref != client_ref);
            self.ledger.forget(&client_ref);
            self.resting_at_venue.remove(&client_ref);
            // A mandate whose legs are gone has no exposure left to correct.
            if !self.legs.iter().any(|other| other.strategy_id == leg.strategy_id) {
                self.drop_neutralizations(&leg.strategy_id);
            }
        }
    }

    fn persist(&self) -> Result<(), PortError> {
        let snapshot = Snapshot {
            positions: self.positions.all(),
            legs: self.legs.clone(),
            attempted_strategies: self.attempted.iter().cloned().collect(),
            client_seq: self.client_seq,
            fill_cursor: self.ledger.cursor(),
            submissions: self.ledger.submissions(),
            absorbed_blocks: self.ledger.absorbed_blocks(),
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
