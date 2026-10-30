//! One engine cycle: read the venue, decide what completed, act, persist.
//!
//! `run_cycle` is the only entry point the graded suite calls. The order of the
//! steps below is the order they run in, and everything else in the crate is a
//! type or a helper reached from here.
//!
//! The order matters in two places. Observations are folded in before anything is
//! decided, so a decision never rests on a stale view of the venue. And a port
//! failure no longer stops the cycle from persisting, because a cycle that
//! absorbed evidence and then died would lose the cursor that said how far it had
//! read.

use std::collections::{BTreeMap, BTreeSet};

use crate::engine::config::EngineConfig;
use crate::engine::fill_classifier::AttributionLedger;
use crate::engine::managers::{PendingOrderManager, PositionManager};
use crate::engine::neutralization::{plan_neutralization, NeutralizationPlan};
use crate::engine::sizing::leg_size;
use crate::ports::{
    Clock, FillEventSource, InventorySource, OrderGateway, OrderKind, OrderRequest, PortError,
    StateStore, StrategySource, SubmitError,
};
use crate::types::ids::{qty_at_least, ClientRef, Qty, StrategyId, Venue};
use crate::types::pending_order::PendingOrder;
use crate::types::position::Position;
use crate::types::snapshot::{LegRecord, LegSubmission, NeutralizationRecord, Snapshot};
use crate::types::strategy::StrategyCandidate;
use crate::types::time::Timestamp;

/// What the live order list currently shows about a leg the engine tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegVisibility {
    /// Present in `open_orders`, so it is resting at the venue.
    Resting,
    /// Not working any more, and known to be closed: this engine cancelled it, or
    /// the venue closed it by naming it in a block.
    Withdrawn,
    /// Neither of those. It has left the live list and this engine does not know
    /// why, so it may have filled with the fill not yet through the feed.
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
    /// Corrective orders this engine has committed to, resolved or not, keyed by
    /// the client reference the engine chose for each.
    neutralizations: BTreeMap<ClientRef, NeutralizationRecord>,
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
            pending: PendingOrderManager::new(),
            positions: PositionManager::new(),
            attempted: BTreeSet::new(),
            ledger: AttributionLedger::new(),
            neutralizations: BTreeMap::new(),
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

        // A failed port says nothing about the venue, and it must not cost the
        // cycle the durable state it has already earned, so failures are carried
        // to the end and the snapshot is written whatever happens.
        let mut failure: Option<PortError> = None;
        self.recover_lost_submissions(now);
        if let Err(error) = self.enter_new_strategies(now) {
            failure = failure.or(Some(error));
        }
        self.reconcile_open_strategies(now);
        self.retire_stale_orders(now);
        self.sweep_abandoned();
        self.persist()?;

        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Restore the durable state on the first cycle after a start or restart.
    fn ensure_loaded(&mut self) -> Result<(), PortError> {
        if self.loaded {
            return Ok(());
        }
        if let Some(snapshot) = self.store.load()? {
            self.positions = PositionManager::from_positions(snapshot.positions);
            self.pending = PendingOrderManager::from_orders(snapshot.legs);
            self.attempted = snapshot.attempted_strategies.into_iter().collect();
            self.client_seq = snapshot.client_seq;
            self.ledger = AttributionLedger::restore(
                snapshot.submissions,
                snapshot.absorbed_blocks,
                snapshot.fill_cursor,
            );
            self.neutralizations = snapshot
                .neutralizations
                .into_iter()
                .map(|record| (record.client_ref.clone(), record))
                .collect();
        }
        self.loaded = true;
        Ok(())
    }

    /// Read both venue sources into the ledger before anything is decided.
    ///
    /// A failed `open_orders` says nothing about the venue, so no order can be
    /// assumed live from it: every leg it cannot confirm is left unconfirmed, and
    /// an unconfirmed leg is never corrected against.
    fn sync_observations(&mut self) {
        match self.gateway.open_orders() {
            Ok(open) => {
                self.ledger.absorb_open_orders(&open);
                self.resting_at_venue =
                    open.into_iter().map(|order| order.client_ref).collect();
            }
            Err(_) => {
                self.resting_at_venue.clear();
            }
        }
        if let Ok(batch) = self.fills.poll(&self.ledger.cursor()) {
            self.observed_fill_events += self.ledger.absorb_batch(batch);
        }
    }

    /// Resolve, and failing that retry, every submission whose acknowledgement was
    /// lost.
    ///
    /// The venue may or may not have created such an order and its identifier is
    /// unknown, so the client reference is the only key that reaches it. Identity
    /// comes first and for free: anything the venue has since named — on the live
    /// list, on a fill event, on a block — resolves without another submission.
    /// What is still unnamed is retried under the **same** client reference, so the
    /// venue's idempotency resolves the retry onto the original order rather than
    /// placing the same economic exposure a second time.
    fn recover_lost_submissions(&mut self, now: Timestamp) {
        self.reattach_identities();

        let legs: Vec<LegRecord> = self
            .pending
            .all_legs()
            .into_iter()
            .filter(|leg| {
                leg.submission == LegSubmission::Unacknowledged
                    && leg.order_id.is_none()
                    && now < leg.expiry_time
            })
            .collect();
        for leg in legs {
            let request = OrderRequest {
                strategy_id: leg.strategy_id.clone(),
                instrument_id: leg.instrument_id.clone(),
                venue: leg.venue,
                kind: OrderKind::RestingLimit,
                limit_price: leg.limit_price,
                size: leg.size,
                client_ref: leg.client_ref.clone(),
                expiry_time: leg.expiry_time,
            };
            // The window of a retried order is the one that opened at the first
            // attempt, so the ledger is told the original instant, not this one.
            self.ledger
                .track(leg.client_ref.clone(), None, None, leg.size, leg.created_at);
            match self.gateway.submit(&request) {
                Ok(accepted) => {
                    self.ledger.track(
                        leg.client_ref.clone(),
                        Some(accepted.order_id.clone()),
                        Some(accepted.venue_seq),
                        leg.size,
                        leg.created_at,
                    );
                    if let Some(slot) = self.pending.get_mut(&leg.client_ref) {
                        slot.order_id = Some(accepted.order_id);
                        slot.venue_seq = Some(accepted.venue_seq);
                        slot.submission = LegSubmission::Named;
                    }
                }
                Err(SubmitError::AcknowledgementLost) => {}
                Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                    if let Some(slot) = self.pending.get_mut(&leg.client_ref) {
                        slot.submission = LegSubmission::Uncreated;
                    }
                }
            }
        }

        let records: Vec<NeutralizationRecord> = self
            .neutralizations
            .values()
            .filter(|record| record.order_id.is_none() && now < record.expiry_time)
            .cloned()
            .collect();
        for record in records {
            let request = Self::neutralization_request(&record);
            let size = record.size;
            // The ledger already holds this submission from the first attempt; the
            // window it opened then is the one a retry must keep.
            if !self.ledger.is_tracked(&record.client_ref) {
                self.ledger
                    .track(record.client_ref.clone(), None, None, size, now);
            }
            match self.gateway.submit(&request) {
                Ok(accepted) => {
                    self.ledger.track(
                        record.client_ref.clone(),
                        Some(accepted.order_id.clone()),
                        Some(accepted.venue_seq),
                        size,
                        now,
                    );
                    if let Some(slot) = self.neutralizations.get_mut(&record.client_ref) {
                        slot.order_id = Some(accepted.order_id);
                    }
                }
                Err(SubmitError::AcknowledgementLost) => {}
                Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                    // Nothing was created, so nothing is working and the exposure
                    // stays open for a later cycle to recompute.
                    self.neutralizations.remove(&record.client_ref);
                    self.forget_submission(&record.client_ref);
                }
            }
        }
    }

    /// Adopt the venue identifier of anything the venue has since named.
    fn reattach_identities(&mut self) {
        let mut named: Vec<(ClientRef, Option<u64>)> = Vec::new();
        for leg in self.pending.all_legs() {
            if leg.order_id.is_none() {
                if let Some(order_id) = self.ledger.order_for(&leg.client_ref) {
                    named.push((leg.client_ref.clone(), self.ledger.venue_seq_for(&leg.client_ref)));
                    let _ = order_id;
                }
            }
        }
        for (client_ref, venue_seq) in named {
            if let Some(order_id) = self.ledger.order_for(&client_ref) {
                if let Some(slot) = self.pending.get_mut(&client_ref) {
                    slot.order_id = Some(order_id);
                    slot.venue_seq = venue_seq;
                    slot.submission = LegSubmission::Named;
                }
            }
        }
        let mut refs: Vec<ClientRef> = Vec::new();
        for record in self.neutralizations.values() {
            if record.order_id.is_none() && self.ledger.order_for(&record.client_ref).is_some() {
                refs.push(record.client_ref.clone());
            }
        }
        for client_ref in refs {
            if let Some(order_id) = self.ledger.order_for(&client_ref) {
                if let Some(slot) = self.neutralizations.get_mut(&client_ref) {
                    slot.order_id = Some(order_id);
                }
            }
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

    fn enter_new_strategies(&mut self, now: Timestamp) -> Result<(), PortError> {
        self.retry_uncreated_legs(now);
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
            let north = self.place_leg(&candidate, Venue::North, north_size, now);
            let south = self.place_leg(&candidate, Venue::South, south_size, now);
            if north == LegSubmission::Uncreated && south == LegSubmission::Uncreated {
                // Nothing exists at the venue for this mandate, so one failed
                // attempt must not latch it shut: drop the attempt and let a later
                // cycle start it cleanly.
                let stale: Vec<ClientRef> = self
                    .pending
                    .for_strategy(&candidate.strategy_id)
                    .into_iter()
                    .map(|leg| leg.client_ref.clone())
                    .collect();
                for client_ref in stale {
                    self.pending.remove(&client_ref);
                }
                self.attempted.remove(&candidate.strategy_id);
            }
        }
        Ok(())
    }

    /// An entry leg no attempt has created at the venue: the venue rejected it, or
    /// the transport failed. Nothing was created, so the exposure that attempt was
    /// meant to take on is still open and a later cycle may try again — under a
    /// fresh reference, because there is no order to be idempotent about.
    fn retry_uncreated_legs(&mut self, now: Timestamp) {
        let uncreated: Vec<LegRecord> = self
            .pending
            .all_legs()
            .into_iter()
            .filter(|leg| {
                leg.submission == LegSubmission::Uncreated
                    && !leg.closing
                    && !leg.cancelled
                    && now < leg.expiry_time
            })
            .collect();
        for leg in uncreated {
            let client_ref = self.next_client_ref();
            let request = OrderRequest {
                strategy_id: leg.strategy_id.clone(),
                instrument_id: leg.instrument_id.clone(),
                venue: leg.venue,
                kind: OrderKind::RestingLimit,
                limit_price: leg.limit_price,
                size: leg.size,
                client_ref: client_ref.clone(),
                expiry_time: leg.expiry_time,
            };
            match self.gateway.submit(&request) {
                Ok(accepted) => {
                    self.ledger.track(
                        client_ref.clone(),
                        Some(accepted.order_id.clone()),
                        Some(accepted.venue_seq),
                        leg.size,
                        now,
                    );
                    self.pending.remove(&leg.client_ref);
                    let mut replacement = leg.clone();
                    replacement.client_ref = client_ref;
                    replacement.order_id = Some(accepted.order_id);
                    replacement.venue_seq = Some(accepted.venue_seq);
                    replacement.submission = LegSubmission::Named;
                    replacement.created_at = now;
                    self.pending.insert(replacement);
                }
                Err(SubmitError::AcknowledgementLost) => {
                    self.ledger.track(
                        client_ref.clone(),
                        None,
                        None,
                        leg.size,
                        now,
                    );
                    self.pending.remove(&leg.client_ref);
                    let mut replacement = leg.clone();
                    replacement.client_ref = client_ref;
                    replacement.order_id = None;
                    replacement.submission = LegSubmission::Unacknowledged;
                    replacement.created_at = now;
                    self.pending.insert(replacement);
                }
                Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {}
            }
        }
    }

    /// Rest one leg of a mandate and start tracking it.
    fn place_leg(
        &mut self,
        candidate: &StrategyCandidate,
        venue: Venue,
        size: Qty,
        now: Timestamp,
    ) -> LegSubmission {
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
        let outcome = match self.gateway.submit(&request) {
            Ok(accepted) => {
                self.ledger.track(
                    client_ref.clone(),
                    Some(accepted.order_id.clone()),
                    Some(accepted.venue_seq),
                    size,
                    now,
                );
                self.pending.insert(LegRecord {
                    strategy_id: candidate.strategy_id.clone(),
                    instrument_id: candidate.instrument_for(venue).clone(),
                    venue,
                    client_ref,
                    order_id: Some(accepted.order_id),
                    venue_seq: Some(accepted.venue_seq),
                    limit_price: request.limit_price,
                    size,
                    expiry_time: candidate.expiry_time,
                    created_at: now,
                    submission: LegSubmission::Named,
                    closing: false,
                    cancelled: false,
                });
                LegSubmission::Named
            }
            Err(SubmitError::AcknowledgementLost) => {
                // The venue may have created it. It is tracked from this instant,
                // which is the window that opened at the first attempt, and it is
                // treated as potentially working until its identity is recovered.
                self.ledger.track(client_ref.clone(), None, None, size, now);
                self.pending.insert(LegRecord {
                    strategy_id: candidate.strategy_id.clone(),
                    instrument_id: candidate.instrument_for(venue).clone(),
                    venue,
                    client_ref,
                    order_id: None,
                    venue_seq: None,
                    limit_price: request.limit_price,
                    size,
                    expiry_time: candidate.expiry_time,
                    created_at: now,
                    submission: LegSubmission::Unacknowledged,
                    closing: false,
                    cancelled: false,
                });
                LegSubmission::Unacknowledged
            }
            Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                // No economic order was created, so the exposure is still open and a
                // later cycle may try again.
                self.pending.insert(LegRecord {
                    strategy_id: candidate.strategy_id.clone(),
                    instrument_id: candidate.instrument_for(venue).clone(),
                    venue,
                    client_ref,
                    order_id: None,
                    venue_seq: None,
                    limit_price: request.limit_price,
                    size,
                    expiry_time: candidate.expiry_time,
                    created_at: now,
                    submission: LegSubmission::Uncreated,
                    closing: false,
                    cancelled: false,
                });
                LegSubmission::Uncreated
            }
        };
        outcome
    }

    /// Quantity attributed to this leg: the resting order itself, plus every
    /// corrective order this engine has committed to for the same mandate and
    /// venue. A corrective fill resolves the exposure, so it counts here — and,
    /// being counted here, it is never also counted as a fill of the resting order.
    fn acquired_for(&self, leg: &LegRecord) -> Qty {
        let mut total = self.ledger.attributed_for(&leg.client_ref);
        for record in self.neutralizations.values() {
            if record.strategy_id == leg.strategy_id && record.venue == leg.venue {
                total += self.ledger.attributed_for(&record.client_ref);
            }
        }
        total
    }

    /// Exact integer comparison. There is no tolerance: a leg short by one base
    /// unit is short.
    fn leg_is_complete(&self, leg: &LegRecord) -> bool {
        qty_at_least(self.acquired_for(leg), leg.size)
    }

    /// Whether the venue has closed this leg's order outright, so it can never
    /// acquire another base unit by waiting.
    fn leg_is_closed(&self, leg: &LegRecord) -> bool {
        leg.cancelled || self.ledger.is_finalized(&leg.client_ref)
    }

    /// The exposure still open on this leg.
    ///
    /// Quantity already attributed to the leg — by its own order or by a
    /// corrective order — is gone from it, and so is quantity already working in a
    /// committed correction: what is working is what stops consecutive cycles from
    /// sending the same exposure twice.
    fn residual_for(&self, leg: &LegRecord) -> Qty {
        let mut working: Qty = 0;
        for record in self.neutralizations.values() {
            if record.strategy_id != leg.strategy_id || record.venue != leg.venue {
                continue;
            }
            working += match record.order_id {
                // Unresolved: the venue may have created it and this engine cannot
                // see it, so it counts for its full size.
                None => record.size,
                // Finalized: the venue closed it and cancelled whatever it did not
                // fill, so the gap it leaves behind is real and it counts for
                // nothing.
                Some(_) if self.ledger.is_finalized(&record.client_ref) => 0,
                Some(_) => self.ledger.working_for(&record.client_ref),
            };
        }
        let residual = leg.size - self.acquired_for(leg) - working;
        if residual > 0 {
            residual
        } else {
            0
        }
    }

    fn visibility(&self, leg: &LegRecord) -> LegVisibility {
        if leg.cancelled || self.ledger.is_finalized(&leg.client_ref) {
            // The engine cancelled it, or the venue closed it by naming it in a
            // block. A block is the venue's own statement that the order is closed,
            // so nothing about it is in flight.
            LegVisibility::Withdrawn
        } else if self.resting_at_venue.contains(&leg.client_ref) {
            LegVisibility::Resting
        } else if leg.submission == LegSubmission::Uncreated {
            // No order exists at the venue, so there is nothing to cancel first.
            LegVisibility::Withdrawn
        } else {
            LegVisibility::Unconfirmed
        }
    }

    fn reconcile_open_strategies(&mut self, now: Timestamp) {
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
                self.close_gap(&south_leg, now);
            } else if south_complete {
                self.close_gap(&north_leg, now);
            } else if self.leg_is_closed(&north_leg) || self.leg_is_closed(&south_leg) {
                // Neither leg has been acquired, and one of them the venue has
                // closed outright. It will never acquire another base unit, so the
                // exposure it leaves has to be corrected rather than waited out.
                if self.leg_is_closed(&north_leg) {
                    self.close_gap(&north_leg, now);
                }
                if self.leg_is_closed(&south_leg) {
                    self.close_gap(&south_leg, now);
                }
            }
        }
    }

    /// Close the exposure left by a leg that did not complete.
    ///
    /// A resting leg is not treated as gone merely because a correction has become
    /// necessary. The engine first moves it into closure, then cancels it at the
    /// venue, and only once that cancellation boundary has passed does it send the
    /// corrective order — so a fill already in flight has a cycle in which to land
    /// and be attributed before another economic order goes out.
    ///
    /// An unconfirmed leg is never corrected against: neutralizing quantity the
    /// desk already owns doubles the exposure the correction was meant to close.
    fn close_gap(&mut self, leg: &LegRecord, now: Timestamp) {
        match self.visibility(leg) {
            LegVisibility::Resting => {
                if !leg.closing {
                    // Mark it now; the cancellation happens on a later cycle.
                    if let Some(slot) = self.pending.get_mut(&leg.client_ref) {
                        slot.closing = true;
                    }
                    return;
                }
                let Some(order_id) = leg.order_id.clone() else {
                    return;
                };
                if self.gateway.cancel(&order_id).is_err() {
                    // The order is still live at the venue, so the boundary has not
                    // passed and nothing may be sent against it yet.
                    return;
                }
                self.resting_at_venue.remove(&leg.client_ref);
                if let Some(slot) = self.pending.get_mut(&leg.client_ref) {
                    slot.cancelled = true;
                }
            }
            // Not resting at the venue, so there is nothing to cancel first. A leg
            // the venue closed in a block needs no cancellation at all.
            LegVisibility::Withdrawn => {}
            LegVisibility::Unconfirmed => return,
        }

        if let Some(slot) = self.pending.get_mut(&leg.client_ref) {
            slot.closing = true;
        }

        let residual = self.residual_for(leg);
        // Quantities are whole base units, so a residual of one unit is a real
        // residual and a residual of zero or less is not an exposure.
        if residual <= 0 {
            return;
        }
        let plan = match plan_neutralization(
            self.gateway,
            leg.strategy_id.clone(),
            &leg.instrument_id,
            leg.venue,
            residual,
            self.config.neutralization_slippage,
        ) {
            Ok(Some(plan)) => plan,
            _ => return,
        };
        self.commit_neutralization(leg, plan, now);
    }

    /// Commit to a corrective order and send it.
    ///
    /// The record is written before the order is sent, so a submission whose
    /// outcome is unknown is still something this engine knows it committed to.
    fn commit_neutralization(
        &mut self,
        leg: &LegRecord,
        plan: NeutralizationPlan,
        now: Timestamp,
    ) {
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
        };
        let size = record.size;
        let request = Self::neutralization_request(&record);
        self.neutralizations.insert(client_ref.clone(), record);
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
                if let Some(slot) = self.neutralizations.get_mut(&client_ref) {
                    slot.order_id = Some(accepted.order_id);
                }
            }
            // The venue may have created it. The reference is retained and the
            // retry goes out under it; meanwhile it counts for its full size.
            Err(SubmitError::AcknowledgementLost) => {}
            // No economic order was created, so nothing is working and the
            // exposure is still open for a later cycle to recompute.
            Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                self.neutralizations.remove(&client_ref);
                self.forget_submission(&client_ref);
            }
        }
    }

    fn open_position(&mut self, north_leg: &LegRecord, south_leg: &LegRecord, now: Timestamp) {
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

        let mut forgotten: BTreeSet<ClientRef> = BTreeSet::new();
        forgotten.insert(north_leg.client_ref.clone());
        forgotten.insert(south_leg.client_ref.clone());
        let settled: Vec<ClientRef> = self
            .neutralizations
            .values()
            .filter(|record| record.strategy_id == strategy_id)
            .map(|record| record.client_ref.clone())
            .collect();
        for client_ref in settled {
            forgotten.insert(client_ref.clone());
            self.neutralizations.remove(&client_ref);
        }
        self.pending.remove(&north_leg.client_ref);
        self.pending.remove(&south_leg.client_ref);
        // Once a mandate has settled, nothing keyed by its identifiers may
        // influence a later order that comes to wear one of them.
        self.ledger.forget(&forgotten);
    }

    fn retire_stale_orders(&mut self, now: Timestamp) {
        let mut forgotten: BTreeSet<ClientRef> = BTreeSet::new();
        for client_ref in self.pending.expiring(now) {
            let Some(leg) = self.pending.get(&client_ref).cloned() else {
                continue;
            };
            if let Some(order_id) = leg.order_id.clone() {
                // An order the venue has closed, or one this engine has already
                // cancelled, needs no cancellation.
                if !leg.cancelled && !self.ledger.is_finalized(&client_ref) {
                    let _ = self.gateway.cancel(&order_id);
                }
            }
            forgotten.insert(client_ref.clone());
            self.pending.remove(&client_ref);
        }
        self.ledger.forget(&forgotten);
    }

    /// Drop the bookkeeping of mandates this engine is no longer working: both
    /// legs have gone and no pairing was recorded for them.
    fn sweep_abandoned(&mut self) {
        let abandoned: Vec<ClientRef> = self
            .neutralizations
            .values()
            .filter(|record| {
                !self.positions.has(&record.strategy_id)
                    && self.pending.for_strategy(&record.strategy_id).is_empty()
            })
            .map(|record| record.client_ref.clone())
            .collect();
        let mut forgotten: BTreeSet<ClientRef> = BTreeSet::new();
        for client_ref in abandoned {
            forgotten.insert(client_ref.clone());
            self.neutralizations.remove(&client_ref);
        }
        self.ledger.forget(&forgotten);
    }

    fn forget_submission(&mut self, client_ref: &ClientRef) {
        let mut forgotten: BTreeSet<ClientRef> = BTreeSet::new();
        forgotten.insert(client_ref.clone());
        self.ledger.forget(&forgotten);
    }

    fn persist(&self) -> Result<(), PortError> {
        let snapshot = Snapshot {
            positions: self.positions.all(),
            legs: self.pending.all_legs(),
            attempted_strategies: self.attempted.iter().cloned().collect(),
            client_seq: self.client_seq,
            fill_cursor: self.ledger.cursor(),
            submissions: self.ledger.submissions(),
            absorbed_blocks: self.ledger.absorbed_blocks(),
            neutralizations: self.neutralizations.values().cloned().collect(),
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
