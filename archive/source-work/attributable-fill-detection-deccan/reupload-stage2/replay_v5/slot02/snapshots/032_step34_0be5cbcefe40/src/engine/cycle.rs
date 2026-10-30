//! One engine cycle: read the venue, decide what completed, act, persist.
//!
//! `run_cycle` is the only entry point the graded suite calls. The order of the
//! steps below is the order they run in, and everything else in the crate is a
//! type or a helper reached from here.

use std::collections::{BTreeMap, BTreeSet};

use crate::engine::config::EngineConfig;
use crate::engine::fill_classifier::{AttributionLedger, RecordFlag};
use crate::engine::managers::PositionManager;
use crate::engine::neutralization::plan_neutralization;
use crate::engine::sizing::leg_size;
use crate::ports::{
    InventorySource, Clock, FillEventSource, OrderGateway, OrderKind, OrderRequest, PortError,
    StrategySource, SubmitError, StateStore,
};
use crate::types::ids::{ClientRef, InstrumentId, OrderId, Qty, StrategyId, Venue};
use crate::types::pending_order::PendingOrder;
use crate::types::position::Position;
use crate::types::snapshot::{
    NeutralizationRecord, OrderRole, Snapshot, SubmissionRecord,
};
use crate::types::time::Timestamp;

/// What the live order list currently shows about a leg the engine tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegVisibility {
    /// Present in `open_orders`.
    Resting,
    /// Absent from `open_orders`, and either this engine cancelled it or the
    /// venue closed it by naming it in a block.
    Withdrawn,
    /// Absent from `open_orders`, and neither of those. It may have filled with
    /// the fill not yet through the feed, so it is never neutralized against.
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
    /// Per-order attribution state, and the feed cursor that produced it. Every
    /// order the engine submitted is here, entry leg or correction, named by the
    /// venue or not.
    ledger: AttributionLedger,
    /// Corrective orders this engine has committed to, resolved or not.
    neutralizations: BTreeMap<ClientRef, NeutralizationRecord>,
    attempted: BTreeSet<StrategyId>,
    /// Client references the last `open_orders` call reported as live, and the
    /// venue identifier each of them wears. Not durable: it is re-read every
    /// cycle and says nothing after a restart.
    live_by_client_ref: BTreeSet<ClientRef>,
    live_by_order_id: BTreeMap<OrderId, ClientRef>,
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
            ledger: AttributionLedger::new(),
            neutralizations: BTreeMap::new(),
            attempted: BTreeSet::new(),
            live_by_client_ref: BTreeSet::new(),
            live_by_order_id: BTreeMap::new(),
            loaded: false,
            client_seq: 0,
            observed_fill_events: 0,
        }
    }

    pub fn positions(&self) -> Vec<Position> {
        self.positions.all()
    }

    /// Entry legs the engine is still working. A leg the venue has not yet named
    /// — one whose acknowledgement was lost — cannot be reported here, because
    /// there is no venue identifier to report it under.
    pub fn pending_orders(&self) -> Vec<PendingOrder> {
        self.ledger
            .entry_legs()
            .into_iter()
            .filter_map(|record| record.as_pending_order())
            .collect()
    }

    pub fn observed_fill_events(&self) -> usize {
        self.observed_fill_events
    }

    pub fn run_cycle(&mut self) -> Result<(), PortError> {
        self.ensure_loaded()?;
        let now = self.clock.now();
        self.sync_observations();
        self.recover_unacknowledged();
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
    /// assumed live from it.
    fn sync_observations(&mut self) {
        match self.gateway.open_orders() {
            Ok(open) => {
                let mut by_client_ref = BTreeSet::new();
                let mut by_order_id = BTreeMap::new();
                for order in &open {
                    by_client_ref.insert(order.client_ref.clone());
                    by_order_id.insert(order.order_id.clone(), order.client_ref.clone());
                }
                self.ledger.absorb_open_orders(&open);
                self.live_by_client_ref = by_client_ref;
                self.live_by_order_id = by_order_id;
            }
            Err(_) => {
                self.live_by_client_ref.clear();
                self.live_by_order_id.clear();
            }
        }
        if let Ok(batch) = self.fills.poll(&self.ledger.cursor()) {
            self.observed_fill_events += self.ledger.absorb_batch(batch);
        }
    }

    /// Retry every submission the venue never named, under the client reference
    /// the engine already chose for it.
    ///
    /// The venue deduplicates on that reference, so a retry reattaches to the
    /// order the first attempt may have created instead of placing the same
    /// economic exposure a second time. The attribution window of the retried
    /// order is the one that opened at the first attempt.
    fn recover_unacknowledged(&mut self) {
        for record in self.ledger.unacknowledged() {
            let request = Self::request_for(&record);
            match self.gateway.submit(&request) {
                Ok(accepted) => {
                    self.ledger
                        .attach(&record.client_ref, accepted.order_id, accepted.venue_seq);
                }
                // Still unknown. Keep the record and try again on a later cycle.
                Err(SubmitError::AcknowledgementLost) => {}
                // Nothing was created by that attempt, so the exposure is still
                // open and will be recomputed. Drop the record so the work is
                // re-planned rather than latched to a reference the venue never
                // saw.
                Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                    self.ledger.forget(&record.client_ref);
                    self.neutralizations.remove(&record.client_ref);
                }
            }
        }
    }

    /// The order request a submission record stands for, used unchanged on every
    /// retry so the venue's idempotency key is the one it first saw.
    fn request_for(record: &SubmissionRecord) -> OrderRequest {
        OrderRequest {
            strategy_id: record.strategy_id.clone(),
            instrument_id: record.instrument_id.clone(),
            venue: record.venue,
            kind: match record.role {
                OrderRole::Entry => OrderKind::RestingLimit,
                OrderRole::Neutralization => OrderKind::ImmediateOrCancel,
            },
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
            if now >= candidate.expiry_time {
                continue;
            }
            if candidate.pair_cost() > self.config.max_pair_cost {
                continue;
            }
            let north_size = leg_size(candidate.north_depth, self.config.max_leg_size);
            let south_size = leg_size(candidate.south_depth, self.config.max_leg_size);
            if north_size < self.config.min_leg_size || south_size < self.config.min_leg_size {
                continue;
            }
            if !self.attempted.contains(&candidate.strategy_id) {
                let notional = (candidate.north_price * north_size as f64)
                    + (candidate.south_price * south_size as f64);
                if notional > available_cash {
                    continue;
                }
                self.attempted.insert(candidate.strategy_id.clone());
            }
            // A leg whose submission created nothing is placed again here: one
            // failed attempt does not latch the mandate shut.
            if !self.ledger.has_entry(&candidate.strategy_id, Venue::North) {
                self.place_leg(&candidate, Venue::North, north_size, now);
            }
            if !self.ledger.has_entry(&candidate.strategy_id, Venue::South) {
                self.place_leg(&candidate, Venue::South, south_size, now);
            }
        }
        Ok(())
    }

    fn place_leg(
        &mut self,
        candidate: &crate::types::strategy::StrategyCandidate,
        venue: Venue,
        size: Qty,
        now: Timestamp,
    ) {
        let instrument_id = candidate.instrument_for(venue).clone();
        let client_ref = self.next_client_ref();
        // The record is written before the order is sent, so a submission whose
        // acknowledgement is lost is still tracked, from the instant of the first
        // attempt, under a reference the retry can reuse.
        self.ledger.track(SubmissionRecord {
            client_ref: client_ref.clone(),
            order_id: None,
            window_start: now,
            venue_seq: None,
            size,
            direct_filled: 0,
            block_filled: 0,
            finalized: false,
            role: OrderRole::Entry,
            strategy_id: candidate.strategy_id.clone(),
            instrument_id: instrument_id.clone(),
            venue,
            limit_price: candidate.price_for(venue),
            expiry_time: candidate.expiry_time,
            closing: false,
            cancelled: false,
            retired: false,
        });
        let request = OrderRequest {
            strategy_id: candidate.strategy_id.clone(),
            instrument_id,
            venue,
            kind: OrderKind::RestingLimit,
            limit_price: candidate.price_for(venue),
            size,
            client_ref: client_ref.clone(),
            expiry_time: candidate.expiry_time,
        };
        match self.gateway.submit(&request) {
            Ok(accepted) => {
                self.ledger
                    .attach(&client_ref, accepted.order_id, accepted.venue_seq);
            }
            // The venue may have created it. Keep the record: a later cycle
            // retries under the same reference, and any observation carrying that
            // reference reaches the record.
            Err(SubmitError::AcknowledgementLost) => {}
            Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                self.ledger.forget(&client_ref);
            }
        }
    }

    /// Committed corrections for this mandate and venue, in a deterministic order.
    fn records_for(&self, strategy_id: &StrategyId, venue: Venue) -> Vec<&NeutralizationRecord> {
        self.neutralizations
            .values()
            .filter(|record| record.strategy_id == *strategy_id && record.venue == venue)
            .collect()
    }

    /// Quantity attributed to this leg: the resting order itself, plus every
    /// corrective order this engine sent for the same mandate and venue. A
    /// neutralization fill resolves the exposure; it is not also a fill of the
    /// original resting order.
    fn acquired_for(&self, leg: &SubmissionRecord) -> Qty {
        let mut total = self.ledger.attributed_for(&leg.client_ref);
        for record in self.records_for(&leg.strategy_id, leg.venue) {
            total += self.ledger.attributed_for(&record.client_ref);
        }
        total
    }

    /// What a committed correction still covers.
    ///
    /// A correction the venue acknowledged contributes what it has left to fill.
    /// One whose acknowledgement was lost contributes its whole size, because the
    /// venue may have created it and the engine cannot see it. One the venue has
    /// closed contributes nothing at all, whatever it managed to fill: the gap it
    /// leaves behind is real.
    fn still_working(&self, record: &NeutralizationRecord) -> Qty {
        match self.ledger.record(&record.client_ref) {
            None => 0,
            Some(submission) if submission.finalized || submission.retired => 0,
            Some(submission) if submission.order_id.is_none() => submission.size,
            Some(submission) => submission.outstanding(),
        }
    }

    /// The exposure on a leg that is still open. The third term is what stops
    /// consecutive cycles from sending the same exposure twice.
    fn residual_for(&self, leg: &SubmissionRecord) -> Qty {
        let mut attributed = self.ledger.attributed_for(&leg.client_ref);
        let mut working: Qty = 0;
        for record in self.records_for(&leg.strategy_id, leg.venue) {
            attributed += self.ledger.attributed_for(&record.client_ref);
            working += self.still_working(record);
        }
        leg.size - attributed - working
    }

    /// Exact integer comparison. There is no tolerance: a leg short by one base
    /// unit is short.
    fn leg_is_complete(&self, leg: &SubmissionRecord) -> bool {
        self.acquired_for(leg) >= leg.size
    }

    /// Whether the venue currently lists this order as live.
    ///
    /// The client reference decides it: an identifier that is live is the order
    /// the engine tracked only if it is this order's own reference wearing it.
    fn is_live(&self, leg: &SubmissionRecord) -> bool {
        if leg.finalized {
            return false;
        }
        if self.live_by_client_ref.contains(&leg.client_ref) {
            return true;
        }
        match &leg.order_id {
            Some(order_id) => self.live_by_order_id.get(order_id) == Some(&leg.client_ref),
            None => false,
        }
    }

    fn visibility(&self, leg: &SubmissionRecord) -> LegVisibility {
        if self.is_live(leg) {
            LegVisibility::Resting
        } else if leg.cancelled || leg.finalized {
            LegVisibility::Withdrawn
        } else {
            LegVisibility::Unconfirmed
        }
    }

    fn entry_leg(&self, strategy_id: &StrategyId, venue: Venue) -> Option<SubmissionRecord> {
        self.ledger
            .entry_legs()
            .into_iter()
            .find(|record| record.strategy_id == *strategy_id && record.venue == venue)
    }

    fn reconcile_open_strategies(&mut self, now: Timestamp) -> Result<(), PortError> {
        for strategy_id in self.ledger.entry_strategy_ids() {
            if self.positions.has(&strategy_id) {
                continue;
            }
            let north_leg = match self.entry_leg(&strategy_id, Venue::North) {
                Some(order) => order,
                None => continue,
            };
            let south_leg = match self.entry_leg(&strategy_id, Venue::South) {
                Some(order) => order,
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
    /// fill a cycle to arrive before another economic order is sent. A leg the
    /// venue has already closed — it named it in a block — needs no cancellation;
    /// its residual is covered directly. An unconfirmed leg is never corrected
    /// at all: neutralizing quantity the desk already owns doubles the exposure.
    fn close_gap(&mut self, leg: &SubmissionRecord, now: Timestamp) -> Result<(), PortError> {
        match self.visibility(leg) {
            LegVisibility::Resting => {
                if !leg.closing {
                    self.ledger.set_flag(&leg.client_ref, RecordFlag::Closing, true);
                    return Ok(());
                }
                let Some(order_id) = leg.order_id.clone() else {
                    return Ok(());
                };
                if self.gateway.cancel(&order_id).is_err() {
                    return Ok(());
                }
                self.ledger
                    .set_flag(&leg.client_ref, RecordFlag::Cancelled, true);
                self.live_by_client_ref.remove(&leg.client_ref);
                self.live_by_order_id.remove(&order_id);
            }
            // Nothing to cancel, or nothing that may be corrected.
            LegVisibility::Withdrawn => {}
            LegVisibility::Unconfirmed => return Ok(()),
        }

        let residual = self.residual_for(leg);
        let plan = plan_neutralization(
            self.gateway,
            &leg.strategy_id,
            &leg.instrument_id,
            leg.venue,
            residual,
            self.config.neutralization_slippage,
        )?;
        let Some(plan) = plan else {
            return Ok(());
        };

        // The record is written before the order is sent, so a submission whose
        // outcome is unknown is still something this engine knows it committed to.
        let client_ref = self.next_client_ref();
        let record = NeutralizationRecord::new(
            plan.strategy_id,
            plan.venue,
            plan.instrument_id,
            client_ref.clone(),
            plan.limit_price,
            plan.size,
            leg.expiry_time,
        );
        self.ledger.track(SubmissionRecord {
            client_ref: client_ref.clone(),
            order_id: None,
            window_start: now,
            venue_seq: None,
            size: plan.size,
            direct_filled: 0,
            block_filled: 0,
            finalized: false,
            role: OrderRole::Neutralization,
            strategy_id: record.strategy_id.clone(),
            instrument_id: record.instrument_id.clone(),
            venue: record.venue,
            limit_price: record.limit_price,
            expiry_time: record.expiry_time,
            closing: false,
            cancelled: false,
            retired: false,
        });
        self.neutralizations.insert(client_ref.clone(), record.clone());

        let request = Self::request_for(self.ledger.record(&client_ref).unwrap());
        match self.gateway.submit(&request) {
            Ok(accepted) => {
                self.ledger
                    .attach(&client_ref, accepted.order_id, accepted.venue_seq);
            }
            // The venue may have created it. Keep the record: its full size counts
            // as working until the venue names it, and a later cycle retries under
            // the same reference.
            Err(SubmitError::AcknowledgementLost) => {}
            // Nothing came back from the venue, so drop the record and let a later
            // cycle work out what is still missing.
            Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                self.ledger.forget(&client_ref);
                self.neutralizations.remove(&client_ref);
            }
        }

        Ok(())
    }

    fn open_position(
        &mut self,
        north_leg: &SubmissionRecord,
        south_leg: &SubmissionRecord,
        now: Timestamp,
    ) {
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
        // The mandate has settled, so nothing keyed by its identifiers may
        // influence a later order that comes to wear one of them.
        self.ledger
            .set_flag(&north_leg.client_ref, RecordFlag::Retired, true);
        self.ledger
            .set_flag(&south_leg.client_ref, RecordFlag::Retired, true);
        let settled: Vec<ClientRef> = self
            .neutralizations
            .values()
            .filter(|record| record.strategy_id == strategy_id)
            .map(|record| record.client_ref.clone())
            .collect();
        for client_ref in settled {
            self.ledger.set_flag(&client_ref, RecordFlag::Retired, true);
            self.neutralizations.remove(&client_ref);
        }
    }

    fn retire_stale_orders(&mut self, now: Timestamp) {
        for record in self.ledger.entry_legs() {
            if !record.should_cancel(now) {
                continue;
            }
            if let Some(order_id) = record.order_id.clone() {
                let _ = self.gateway.cancel(&order_id);
                self.live_by_order_id.remove(&order_id);
            }
            self.live_by_client_ref.remove(&record.client_ref);
            self.ledger
                .set_flag(&record.client_ref, RecordFlag::Retired, true);
        }
    }

    fn persist(&self) -> Result<(), PortError> {
        let snapshot = Snapshot {
            positions: self.positions.all(),
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
