//! Per-order fill attribution.
//!
//! Custody inventory is per instrument, shared and lagging, so it cannot show that
//! one specific order filled. This ledger keeps one record per order the engine
//! itself submitted and accumulates against that record what the venue says about
//! it, from three sources:
//!
//! - per-order fill events delivered by `FillEventSource::poll`;
//! - block matches, which name several of the engine's orders at once and report
//!   only a total;
//! - `filled_qty` on the entries of `OrderGateway::open_orders`.
//!
//! `/app/spec/venue-semantics.md` is the authority on what each of those three
//! sources means, on the attribution window, and on how a block total divides
//! across the orders it covers.
//!
//! The ledger also owns the feed cursor, because the cursor and the state it
//! produced have to be saved and restored together.
//!
//! Records are keyed by `client_ref`, the one key the engine always knows and the
//! only one that survives a lost acknowledgement. The venue's `order_id` is
//! learned later if at all, and the venue recycles it once a window has closed,
//! so identity is resolved through the client reference wherever one is
//! available and through the identifier only under the window that makes it
//! meaningful.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use crate::ports::{BlockFill, FillBatch, FillEvent, OpenOrder};
use crate::types::ids::{proportional_share, BlockRef, ClientRef, OrderId, Qty, StrategyId, Venue};
use crate::types::snapshot::{Cursor, OrderRole, SubmissionRecord};
use crate::types::time::Timestamp;

#[derive(Debug, Clone, Default)]
pub struct AttributionLedger {
    records: BTreeMap<ClientRef, SubmissionRecord>,
    absorbed_blocks: BTreeSet<BlockRef>,
    cursor: Cursor,
}

impl AttributionLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuild a ledger from the fields a `Snapshot` carried across a restart.
    pub fn restore(
        submissions: Vec<SubmissionRecord>,
        absorbed_blocks: Vec<BlockRef>,
        cursor: Cursor,
    ) -> Self {
        let mut records = BTreeMap::new();
        for record in submissions {
            records.insert(record.client_ref.clone(), record);
        }
        Self {
            records,
            absorbed_blocks: absorbed_blocks.into_iter().collect(),
            cursor,
        }
    }

    /// The three pieces of ledger state a `Snapshot` has to carry.
    pub fn cursor(&self) -> Cursor {
        self.cursor.clone()
    }

    pub fn submissions(&self) -> Vec<SubmissionRecord> {
        self.records.values().cloned().collect()
    }

    pub fn absorbed_blocks(&self) -> Vec<BlockRef> {
        self.absorbed_blocks.iter().cloned().collect()
    }

    /// Every record, in a deterministic order.
    pub fn records(&self) -> Vec<SubmissionRecord> {
        self.records.values().cloned().collect()
    }

    /// Record an order the engine has just submitted, opening its window now.
    ///
    /// Called again for the same `client_ref` — which is what a resubmission
    /// looks like — this leaves the existing attribution state and, above all,
    /// the original window start alone.
    pub fn track(&mut self, record: SubmissionRecord) {
        self.records.entry(record.client_ref.clone()).or_insert(record);
    }

    pub fn forget(&mut self, client_ref: &ClientRef) -> Option<SubmissionRecord> {
        self.records.remove(client_ref)
    }

    /// The venue has named this order. A retry under the same client reference
    /// does not reopen the window; only the identifiers are filled in.
    pub fn attach(&mut self, client_ref: &ClientRef, order_id: OrderId, venue_seq: u64) {
        if let Some(record) = self.records.get_mut(client_ref) {
            if record.order_id.is_none() {
                record.order_id = Some(order_id);
            }
            if record.venue_seq.is_none() {
                record.venue_seq = Some(venue_seq);
            }
        }
    }

    /// Quantity attributed to one submission so far.
    pub fn attributed_for(&self, client_ref: &ClientRef) -> Qty {
        self.record(client_ref)
            .map(SubmissionRecord::attributed)
            .unwrap_or(0)
    }

    /// Quantity this submission could still acquire. A submission the venue has
    /// closed can acquire nothing more, whatever it managed to fill.
    pub fn outstanding_for(&self, client_ref: &ClientRef) -> Qty {
        match self.record(client_ref) {
            Some(record) if record.finalized => 0,
            Some(record) => record.outstanding(),
            None => 0,
        }
    }

    /// The venue's id for a submission, once anything has reported one.
    pub fn order_for(&self, client_ref: &ClientRef) -> Option<OrderId> {
        self.record(client_ref).and_then(|record| record.order_id.clone())
    }

    pub fn record(&self, client_ref: &ClientRef) -> Option<&SubmissionRecord> {
        self.records.get(client_ref)
    }

    pub fn record_mut(&mut self, client_ref: &ClientRef) -> Option<&mut SubmissionRecord> {
        self.records.get_mut(client_ref)
    }

    /// Records the engine has not yet had named by the venue, oldest first.
    ///
    /// These are the submissions whose acknowledgement was lost: the venue may
    /// have created them, so they are still worked, and the engine retries them
    /// under the client reference it already chose.
    pub fn unacknowledged(&self) -> Vec<SubmissionRecord> {
        self.records
            .values()
            .filter(|record| record.order_id.is_none() && !record.retired && !record.finalized)
            .cloned()
            .collect()
    }

    /// Entry legs of mandates the engine is still working, in a deterministic
    /// order.
    pub fn entry_legs(&self) -> Vec<SubmissionRecord> {
        self.records
            .values()
            .filter(|record| record.role == OrderRole::Entry && !record.retired)
            .cloned()
            .collect()
    }

    pub fn has_entry(&self, strategy_id: &StrategyId, venue: Venue) -> bool {
        self.records.values().any(|record| {
            record.role == OrderRole::Entry
                && !record.retired
                && record.strategy_id == *strategy_id
                && record.venue == venue
        })
    }

    /// Strategies with at least one entry leg still being tracked.
    pub fn entry_strategy_ids(&self) -> Vec<StrategyId> {
        let mut ids: Vec<StrategyId> = self
            .entry_legs()
            .into_iter()
            .map(|record| record.strategy_id)
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    pub fn set_flag(&mut self, client_ref: &ClientRef, field: RecordFlag, value: bool) {
        if let Some(record) = self.records.get_mut(client_ref) {
            match field {
                RecordFlag::Closing => record.closing = value,
                RecordFlag::Cancelled => record.cancelled = value,
                RecordFlag::Retired => record.retired = value,
            }
        }
    }

    /// Resolve an observation to one of our submissions.
    ///
    /// The client reference is the engine's own key and is authoritative, so it
    /// wins. Failing that, the venue's identifier is used — but only under the
    /// attribution window that makes it meaningful, because the venue recycles
    /// identifiers once a window has closed.
    fn resolve(
        &mut self,
        order_id: &OrderId,
        client_ref: &ClientRef,
        at: Option<Timestamp>,
    ) -> Option<ClientRef> {
        if self.records.contains_key(client_ref) {
            let record = self.records.get_mut(client_ref)?;
            if record.order_id.is_none() {
                record.order_id = Some(order_id.clone());
            }
            return Some(client_ref.clone());
        }
        self.resolve_by_order(order_id, at)
    }

    /// Find the submission currently wearing a venue identifier.
    ///
    /// With no timestamp — which is the case for the live order list, an
    /// identifier that is live is by definition the order the engine tracked —
    /// the window is not applied. With one, a record whose window contains the
    /// instant is preferred; among equals the most recently opened window wins,
    /// because a recycled identifier belongs to the later order.
    fn resolve_by_order(&self, order_id: &OrderId, at: Option<Timestamp>) -> Option<ClientRef> {
        let mut best: Option<(u32, Reverse<Timestamp>, ClientRef)> = None;
        for record in self.records.values() {
            if record.retired || record.order_id.as_ref() != Some(order_id) {
                continue;
            }
            // A record whose window contains the instant is the order the
            // identifier belongs to; among equals the later window wins, because
            // a recycled identifier belongs to the later order.
            let mut rank = match at {
                Some(instant) if record.window_contains(instant) => 0,
                Some(_) => 1,
                None => 0,
            };
            if record.finalized {
                rank += 2;
            }
            let candidate = (
                rank,
                Reverse(record.window_start),
                record.client_ref.clone(),
            );
            let better = match &best {
                None => true,
                Some(current) => candidate < *current,
            };
            if better {
                best = Some(candidate);
            }
        }
        best.map(|(_, _, client_ref)| client_ref)
    }

    /// Fold one polled batch into the ledger and advance the cursor.
    ///
    /// Returns the number of per-order events in the batch, which the engine
    /// reports through `Engine::observed_fill_events`.
    pub fn absorb_batch(&mut self, batch: FillBatch) -> usize {
        let count = batch.events.len();
        for event in batch.events {
            self.absorb_event(event);
        }
        for block in batch.blocks {
            self.absorb_block(block);
        }
        self.cursor = batch.next_cursor;
        count
    }

    /// Direct progress is cumulative on the wire, so it is a maximum here: the
    /// same figure seen again, or seen from the live list as well, is the same
    /// base units rather than more of them.
    fn absorb_event(&mut self, event: FillEvent) {
        let Some(key) = self.resolve(&event.order_id, &event.client_ref, Some(event.at)) else {
            return;
        };
        let Some(record) = self.records.get_mut(&key) else {
            return;
        };
        if !record.window_contains(event.at) {
            return;
        }
        if event.cumulative_filled_qty > record.direct_filled {
            record.direct_filled = event.cumulative_filled_qty;
        }
    }

    /// Divide one block match across the orders it covers.
    ///
    /// The venue reports only a total and names the orders it was matched
    /// against; the division rule is in `/app/spec/venue-semantics.md`. Every step
    /// is integer arithmetic, and any part of the block that no tracked order can
    /// account for is dropped rather than assigned.
    pub fn absorb_block(&mut self, block: BlockFill) {
        // The same match, redelivered, is the same match.
        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }

        // Resolve every covered identifier first: a block is final for all of
        // them, whether or not the division gives them anything.
        let mut covered: Vec<ClientRef> = Vec::new();
        for order_id in &block.covers {
            if let Some(key) = self.resolve_by_order(order_id, Some(block.at)) {
                if !covered.contains(&key) {
                    covered.push(key);
                }
            }
        }

        // The eligible set, carrying everything the ordering rule needs so that
        // no container's iteration order can decide the outcome.
        let mut eligible: Vec<(Timestamp, u64, OrderId, ClientRef, Qty)> = Vec::new();
        for key in &covered {
            let Some(record) = self.records.get(key) else {
                continue;
            };
            if !record.window_contains(block.at) {
                continue;
            }
            let outstanding = self.outstanding_for(key);
            if outstanding <= 0 {
                continue;
            }
            eligible.push((
                record.window_start,
                record.venue_seq.unwrap_or(0),
                record.order_id.clone().unwrap_or_else(|| OrderId::new("")),
                key.clone(),
                outstanding,
            ));
        }
        eligible.sort();

        let basis: Qty = eligible.iter().map(|entry| entry.4).sum();
        if basis > 0 {
            // Quantity above the basis belongs to orders this engine does not track.
            let divisible = if block.block_qty < basis {
                block.block_qty
            } else {
                basis
            };
            if divisible > 0 {
                let mut assigned: Qty = 0;
                let mut shares: Vec<Qty> = Vec::with_capacity(eligible.len());
                for entry in &eligible {
                    let share = proportional_share(divisible, entry.4, basis);
                    assigned += share;
                    shares.push(share);
                }

                // Truncation leaves a few base units unassigned. They go one each
                // to the first orders in the ordering from the division rule.
                let remainder = divisible - assigned;
                if remainder > 0 {
                    for index in 0..(remainder as usize).min(shares.len()) {
                        shares[index] += 1;
                    }
                }

                for (entry, share) in eligible.iter().zip(shares) {
                    if let Some(record) = self.records.get_mut(&entry.3) {
                        record.block_filled += share;
                    }
                }
            }
        }

        // A block is final for every order it names, whatever it gave them.
        for key in covered {
            if let Some(record) = self.records.get_mut(&key) {
                record.finalized = true;
            }
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp: `open_orders` is point-in-time truth
    /// about what is live right now, and it also supplies a `venue_seq` for an
    /// order the engine had not been told one for. It is also the way an order
    /// whose acknowledgement was lost is given its identifier back.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(key) =
                self.resolve(&open_order.order_id, &open_order.client_ref, None)
            else {
                continue;
            };
            let Some(record) = self.records.get_mut(&key) else {
                continue;
            };
            if record.venue_seq.is_none() {
                record.venue_seq = Some(open_order.venue_seq);
            }
            if open_order.filled_qty > record.direct_filled {
                record.direct_filled = open_order.filled_qty;
            }
        }
    }
}
