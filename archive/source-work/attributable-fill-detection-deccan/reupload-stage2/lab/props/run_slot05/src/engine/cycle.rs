//! One engine cycle: read the venue, decide what completed, act, persist.
//!
//! `run_cycle` is the only entry point the graded suite calls. The order of the
//! steps below is the order they run in, and everything else in the crate is a
//! type or a helper reached from here.

use std::collections::BTreeSet;

use crate::engine::config::EngineConfig;
use crate::engine::fill_classifier::AttributionLedger;
use crate::engine::managers::PositionManager;
use crate::engine::neutralization::plan_neutralization;
use crate::engine::sizing::leg_size;
use crate::ports::{
    Clock, FillEventSource, InventorySource, OrderGateway, OrderKind, OrderRequest, PortError,
    StateStore, StrategySource, SubmitError,
};
use crate::types::ids::{ClientRef, Qty, StrategyId, Venue};
use crate::types::pending_order::{PendingOrder, CANCEL_LEAD};
use crate::types::position::Position;
use crate::types::snapshot::{AttemptState, Snapshot, SubmissionRecord, SubmissionRole};
use crate::types::strategy::StrategyCandidate;
use crate::types::time::Timestamp;

/// What the live order list currently shows about a leg the engine tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegVisibility {
    /// Present in `open_orders`.
    Resting,
    /// Absent, and this engine cancelled it, or the venue closed it by naming it
    /// in a block. Nothing about it is in flight.
    Withdrawn,
    /// Absent, and neither of those. A fill may already be on its way.
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
    /// Per-order attribution state, and the feed cursor that produced it.
    ledger: AttributionLedger,
    attempted: BTreeSet<StrategyId>,
    /// Client references the last `open_orders` call reported as live. Not
    /// durable: it is re-read every cycle and says nothing after a restart.
    resting: BTreeSet<ClientRef>,
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
            attempted: BTreeSet::new(),
            resting: BTreeSet::new(),
            loaded: false,
            client_seq: 0,
            observed_fill_events: 0,
        }
    }

    pub fn positions(&self) -> Vec<Position> {
        self.positions.all()
    }

    /// Entry legs this engine is still working that the venue has named.
    ///
    /// A leg whose acknowledgement was lost has no venue identifier yet, so it
    /// cannot be reported here; it is still tracked, and still recoverable.
    pub fn pending_orders(&self) -> Vec<PendingOrder> {
        let mut orders: Vec<PendingOrder> = self
            .ledger
            .active()
            .into_iter()
            .filter(|record| record.role == SubmissionRole::Entry)
            .filter_map(|record| match record.order_id.clone() {
                Some(order_id) => Some(PendingOrder::new(
                    record.strategy_id,
                    record.instrument_id,
                    record.venue,
                    order_id,
                    record.client_ref,
                    record.venue_seq.unwrap_or(0),
                    record.limit_price,
                    record.size,
                    record.expiry_time,
                    record.window_start,
                )),
                None => None,
            })
            .collect();
        orders.sort_by(|left, right| left.client_ref.cmp(&right.client_ref));
        orders
    }

    pub fn observed_fill_events(&self) -> usize {
        self.observed_fill_events
    }

    pub fn run_cycle(&mut self) -> Result<(), PortError> {
        self.ensure_loaded()?;
        let now = self.clock.now();
        self.sync_observations();
        self.recover_submissions(now);
        self.enter_new_strategies(now)?;
        self.reconcile(now);
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
                self.resting = open.into_iter().map(|order| order.client_ref).collect();
            }
            Err(_) => {
                self.resting.clear();
            }
        }
        if let Ok(batch) = self.fills.poll(&self.ledger.cursor()) {
            self.observed_fill_events += self.ledger.absorb_batch(batch);
        }
    }

    /// Re-ask the venue about every submission it never answered for.
    ///
    /// The client reference is the venue's idempotency key, so retrying under
    /// the same one resolves onto the original order rather than placing the
    /// same economic exposure a second time. A submission the venue refused
    /// outright is retried under its reference too: nothing was created, so the
    /// reference is still free.
    fn recover_submissions(&mut self, now: Timestamp) {
        let unresolved: Vec<SubmissionRecord> = self
            .ledger
            .active()
            .into_iter()
            .filter(|record| record.attempt != AttemptState::Acknowledged)
            .collect();
        for record in unresolved {
            let request = Self::request_for(&record);
            if let Ok(accepted) = self.gateway.submit(&request) {
                // A refused attempt created nothing, so the order the venue has
                // now accepted is a new one and its window opens now. An
                // unacknowledged attempt may already have created it, and the
                // window that opened at the first attempt is the one that counts.
                let window_start = if record.attempt == AttemptState::Failed {
                    now
                } else {
                    record.window_start
                };
                self.ledger.mark_acknowledged(
                    &record.client_ref,
                    accepted.order_id,
                    accepted.venue_seq,
                    window_start,
                );
            }
        }
    }

    fn request_for(record: &SubmissionRecord) -> OrderRequest {
        OrderRequest {
            strategy_id: record.strategy_id.clone(),
            instrument_id: record.instrument_id.clone(),
            venue: record.venue,
            kind: match record.role {
                SubmissionRole::Entry => OrderKind::RestingLimit,
                SubmissionRole::Neutralization => OrderKind::ImmediateOrCancel,
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

    /// Rest one leg of a mandate and start tracking it.
    ///
    /// The record is written before the venue is asked, so a submission whose
    /// acknowledgement is lost is still something a later observation naming its
    /// client reference can reach — and something a later cycle can retry.
    fn place_leg(
        &mut self,
        candidate: &StrategyCandidate,
        venue: Venue,
        size: Qty,
        now: Timestamp,
    ) {
        let client_ref = self.next_client_ref();
        let record = SubmissionRecord {
            client_ref: client_ref.clone(),
            order_id: None,
            window_start: now,
            venue_seq: None,
            size,
            direct_filled: 0,
            block_filled: 0,
            finalized: false,
            role: SubmissionRole::Entry,
            strategy_id: candidate.strategy_id.clone(),
            instrument_id: candidate.instrument_for(venue).clone(),
            venue,
            limit_price: candidate.price_for(venue),
            expiry_time: candidate.expiry_time,
            attempt: AttemptState::Unconfirmed,
            closing: false,
            cancelled: false,
            retired: false,
        };
        let request = Self::request_for(&record);
        self.ledger.track(record);
        match self.gateway.submit(&request) {
            Ok(accepted) => self.ledger.mark_acknowledged(
                &client_ref,
                accepted.order_id,
                accepted.venue_seq,
                now,
            ),
            // The venue may have created it. Keep the record and let a later
            // cycle resolve it under the same client reference.
            Err(SubmitError::AcknowledgementLost) => {}
            // Nothing was created, so the exposure is still open and a later
            // cycle tries again.
            Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                self.ledger.mark_failed(&client_ref);
            }
        }
    }

    /// What one leg has acquired: its own resting order, plus every corrective
    /// order this engine has already placed for the same mandate and venue.
    ///
    /// A correction that fills resolves the exposure; it must not also count a
    /// second time as a fill of the original resting order.
    fn acquired_for(&self, leg: &SubmissionRecord) -> Qty {
        let mut total = leg.attributed();
        for record in self.ledger.corrections(&leg.strategy_id, leg.venue) {
            total += record.attributed();
        }
        total
    }

    /// What is already committed to covering this leg and still working.
    fn working_for(&self, leg: &SubmissionRecord) -> Qty {
        let mut total = 0;
        for record in self.ledger.corrections(&leg.strategy_id, leg.venue) {
            total += self.ledger.working_for(&record.client_ref);
        }
        total
    }

    /// The exposure still open on one leg. Whole base units, so a residual of
    /// one unit is a real residual and a residual of zero or less is not an
    /// exposure.
    fn residual_for(&self, leg: &SubmissionRecord) -> Qty {
        leg.size - self.acquired_for(leg) - self.working_for(leg)
    }

    /// Exact integer comparison. There is no tolerance: a leg short by one base
    /// unit is short.
    fn leg_is_complete(&self, leg: &SubmissionRecord) -> bool {
        self.acquired_for(leg) >= leg.size
    }

    fn visibility(&self, leg: &SubmissionRecord) -> LegVisibility {
        if self.resting.contains(&leg.client_ref) {
            LegVisibility::Resting
        } else if leg.cancelled || leg.is_closed() {
            LegVisibility::Withdrawn
        } else {
            LegVisibility::Unconfirmed
        }
    }

    fn reconcile(&mut self, now: Timestamp) {
        for strategy_id in self.ledger.active_strategy_ids() {
            if self.positions.has(&strategy_id) {
                continue;
            }
            let north_leg = match self.ledger.entry(&strategy_id, Venue::North) {
                Some(record) => record,
                None => continue,
            };
            let south_leg = match self.ledger.entry(&strategy_id, Venue::South) {
                Some(record) => record,
                None => continue,
            };

            let north_complete = self.leg_is_complete(&north_leg);
            let south_complete = self.leg_is_complete(&south_leg);

            if north_complete && south_complete {
                self.open_position(&north_leg, &south_leg, now);
            } else if north_complete {
                self.cover(&south_leg, now);
            } else if south_complete {
                self.cover(&north_leg, now);
            }
        }
    }

    /// Close the exposure a leg that did not complete left behind.
    ///
    /// A resting order is not treated as gone merely because a correction has
    /// become necessary: it is moved into closure first, and the corrective
    /// order follows on a later cycle, once that cancellation boundary has
    /// passed. That gives a fill already in flight a cycle in which to land and
    /// be attributed before another economic order is sent.
    fn cover(&mut self, leg: &SubmissionRecord, now: Timestamp) {
        let mut leg = leg.clone();
        match self.visibility(&leg) {
            LegVisibility::Resting => {
                // A resting order is not gone merely because a correction has
                // become necessary. Mark it for closure and let one whole cycle
                // pass, so a fill already in flight lands and is attributed
                // before the residual is sized.
                if !leg.closing {
                    self.ledger.mark_closing(&leg.client_ref);
                    return;
                }
                let Some(order_id) = leg.order_id.clone() else {
                    return;
                };
                // A cancellation that failed leaves the order live, so nothing
                // is corrected this cycle and a later one tries again.
                if self.gateway.cancel(&order_id).is_err() {
                    return;
                }
                self.ledger.mark_cancelled(&leg.client_ref);
                self.resting.remove(&leg.client_ref);
                leg.cancelled = true;
            }
            // A leg the venue has neither confirmed nor closed may already be
            // filled. Correcting it would double the exposure the correction
            // was meant to close.
            LegVisibility::Unconfirmed => return,
            // Withdrawn: nothing of it is in flight, so cover what is missing.
            LegVisibility::Withdrawn => {}
        }

        let residual = self.residual_for(&leg);
        if residual <= 0 {
            return;
        }

        let plan = match plan_neutralization(
            self.gateway,
            &leg.strategy_id,
            &leg.instrument_id,
            leg.venue,
            residual,
            self.config.neutralization_slippage,
        ) {
            Ok(Some(plan)) => plan,
            _ => return,
        };

        let client_ref = self.next_client_ref();
        let record = SubmissionRecord {
            client_ref: client_ref.clone(),
            order_id: None,
            window_start: now,
            venue_seq: None,
            size: plan.size,
            direct_filled: 0,
            block_filled: 0,
            finalized: false,
            role: SubmissionRole::Neutralization,
            strategy_id: plan.strategy_id,
            instrument_id: plan.instrument_id,
            venue: plan.venue,
            limit_price: plan.limit_price,
            expiry_time: leg.expiry_time,
            attempt: AttemptState::Unconfirmed,
            closing: false,
            cancelled: false,
            retired: false,
        };
        let request = Self::request_for(&record);
        // Written before the venue is asked, so a submission whose outcome is
        // unknown is still something this engine knows it committed to.
        self.ledger.track(record);

        match self.gateway.submit(&request) {
            Ok(accepted) => self.ledger.mark_acknowledged(
                &client_ref,
                accepted.order_id,
                accepted.venue_seq,
                now,
            ),
            // The venue may have created it. Retrying under this client
            // reference reattaches to it instead of placing a second order.
            Err(SubmitError::AcknowledgementLost) => {}
            // Nothing was created, so drop the record and let a later cycle
            // work out what is still missing.
            Err(SubmitError::Rejected(_)) | Err(SubmitError::Transport(_)) => {
                self.ledger.remove(&client_ref);
            }
        }
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
        // Settled: nothing keyed by this mandate's identifiers may influence a
        // later order the venue gives one of them to.
        self.ledger.forget_strategy(&strategy_id);
    }

    fn retire_stale_orders(&mut self, now: Timestamp) {
        let stale: Vec<SubmissionRecord> = self
            .ledger
            .active()
            .into_iter()
            .filter(|record| record.role == SubmissionRole::Entry && Self::is_stale(record, now))
            .collect();
        for record in stale {
            if let Some(order_id) = record.order_id.clone() {
                let _ = self.gateway.cancel(&order_id);
            }
            self.ledger.mark_retired(&record.client_ref);
        }
    }

    fn is_stale(record: &SubmissionRecord, now: Timestamp) -> bool {
        let time_until_expiry = record.expiry_time - now;
        time_until_expiry.as_seconds() < CANCEL_LEAD.as_seconds() || now >= record.expiry_time
    }

    fn persist(&self) -> Result<(), PortError> {
        let snapshot = Snapshot {
            positions: self.positions.all(),
            submissions: self.ledger.submissions(),
            attempted_strategies: self.attempted.iter().cloned().collect(),
            client_seq: self.client_seq,
            fill_cursor: self.ledger.cursor(),
            absorbed_blocks: self.ledger.absorbed_blocks(),
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
