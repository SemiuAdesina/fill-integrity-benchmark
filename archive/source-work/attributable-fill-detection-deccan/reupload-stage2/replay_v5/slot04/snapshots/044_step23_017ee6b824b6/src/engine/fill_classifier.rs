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

use std::collections::{BTreeMap, BTreeSet};

use crate::ports::{BlockFill, FillBatch, FillEvent, OpenOrder};
use crate::types::ids::{proportional_share, BlockRef, ClientRef, OrderId, Qty};
use crate::types::pending_order::ATTRIBUTION_WINDOW;
use crate::types::snapshot::{Cursor, SubmissionRecord};
use crate::types::time::Timestamp;

#[derive(Debug, Clone, Default)]
pub struct AttributionLedger {
    submissions: Vec<SubmissionRecord>,
    index_by_client_ref: BTreeMap<ClientRef, usize>,
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
        let mut ledger = Self {
            submissions,
            index_by_client_ref: BTreeMap::new(),
            absorbed_blocks: absorbed_blocks.into_iter().collect(),
            cursor,
        };
        ledger.reindex();
        ledger
    }

    /// The three pieces of ledger state a `Snapshot` has to carry.
    pub fn cursor(&self) -> Cursor {
        self.cursor.clone()
    }

    pub fn submissions(&self) -> Vec<SubmissionRecord> {
        self.submissions.clone()
    }

    pub fn absorbed_blocks(&self) -> Vec<BlockRef> {
        self.absorbed_blocks.iter().cloned().collect()
    }

    fn reindex(&mut self) {
        self.index_by_client_ref = self
            .submissions
            .iter()
            .enumerate()
            .map(|(index, record)| (record.client_ref.clone(), index))
            .collect();
    }

    /// Record an order the engine has just submitted, opening its window now.
    ///
    /// Called again for the same `client_ref` — which is what a resubmission
    /// looks like — this fills in whatever the venue has since supplied and
    /// leaves the window start and the accumulated attribution alone, because a
    /// retried order's window is the one that opened at the first attempt.
    pub fn track(
        &mut self,
        client_ref: ClientRef,
        order_id: Option<OrderId>,
        venue_seq: Option<u64>,
        size: Qty,
        window_start: Timestamp,
    ) {
        if let Some(index) = self.index_by_client_ref.get(&client_ref).copied() {
            let record = &mut self.submissions[index];
            if record.order_id.is_none() {
                record.order_id = order_id;
            }
            if record.venue_seq.is_none() {
                record.venue_seq = venue_seq;
            }
            return;
        }
        self.index_by_client_ref
            .insert(client_ref.clone(), self.submissions.len());
        self.submissions.push(SubmissionRecord {
            client_ref,
            order_id,
            window_start,
            venue_seq,
            size,
            direct_filled: 0,
            block_filled: 0,
            finalized: false,
        });
    }

    /// Quantity attributed to one submission so far: direct progress plus block
    /// progress.
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

    /// Whether a block has closed this submission.
    pub fn is_finalized(&self, client_ref: &ClientRef) -> bool {
        self.record(client_ref).map_or(false, |record| record.finalized)
    }

    /// The venue's id for a submission, once anything has reported one.
    pub fn order_for(&self, client_ref: &ClientRef) -> Option<OrderId> {
        self.record(client_ref).and_then(|record| record.order_id.clone())
    }

    /// The venue tick that accepted a submission, once anything has reported one.
    pub fn venue_seq_for(&self, client_ref: &ClientRef) -> Option<u64> {
        self.record(client_ref).and_then(|record| record.venue_seq)
    }

    fn record(&self, client_ref: &ClientRef) -> Option<&SubmissionRecord> {
        self.index_by_client_ref
            .get(client_ref)
            .and_then(|index| self.submissions.get(*index))
    }

    /// Whether a venue match timestamp falls in this record's attribution window.
    ///
    /// The window is half-open: `[window_start, window_start + ATTRIBUTION_WINDOW)`.
    /// An observation stamped exactly at the closing instant is outside it, and
    /// so is anything later — the venue is free to have recycled the identifier
    /// by then, so counting one is a misattribution rather than a rounding matter.
    fn in_window(record: &SubmissionRecord, at: Timestamp) -> bool {
        at >= record.window_start && at < record.window_start + ATTRIBUTION_WINDOW
    }

    /// Resolve an observation to one of our submissions.
    ///
    /// The `client_ref` is tried first, because it is the only key the engine
    /// always knows and the only one that is unambiguous when the venue has
    /// recycled an identifier: a later order may wear an id an earlier order
    /// held, and an observation names the two together. Falling back to the
    /// venue id is what lets an observation reach a submission the venue never
    /// acknowledged, whose id is still unknown.
    fn resolve(
        &mut self,
        order_id: &OrderId,
        client_ref: &ClientRef,
        at: Option<Timestamp>,
    ) -> Option<usize> {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            let record = &mut self.submissions[index];
            if record.order_id.is_none() {
                record.order_id = Some(order_id.clone());
            }
            return Some(index);
        }
        self.best_by_order_id(order_id, at)
    }

    /// The submission an identifier speaks for, when more than one carries it.
    ///
    /// The venue hands the same `order_id` to unrelated orders once a window has
    /// closed, and durable state keeps the records of settled orders, so an
    /// identifier alone can be ambiguous. The one still inside its window is the
    /// order that holds the identifier now; failing that, the one the venue has
    /// not closed, and failing that the most recently opened. Every tie is broken
    /// deterministically, so the outcome never depends on where a record happens
    /// to sit in the container.
    fn best_by_order_id(&self, order_id: &OrderId, at: Option<Timestamp>) -> Option<usize> {
        let mut best: Option<(usize, &SubmissionRecord)> = None;
        for (index, record) in self.submissions.iter().enumerate() {
            if record.order_id.as_ref() != Some(order_id) {
                continue;
            }
            let better = match best {
                None => true,
                Some((_, held)) => Self::prefer(record, held, at),
            };
            if better {
                best = Some((index, record));
            }
        }
        best.map(|(index, _)| index)
    }

    fn prefer(
        candidate: &SubmissionRecord,
        held: &SubmissionRecord,
        at: Option<Timestamp>,
    ) -> bool {
        let candidate_in = at.map_or(false, |at| Self::in_window(candidate, at));
        let held_in = at.map_or(false, |at| Self::in_window(held, at));
        if candidate_in != held_in {
            return candidate_in;
        }
        if candidate.finalized != held.finalized {
            return !candidate.finalized;
        }
        if candidate.window_start != held.window_start {
            return candidate.window_start > held.window_start;
        }
        candidate.client_ref < held.client_ref
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

    /// Direct progress is cumulative: what a fill event reports is the order's
    /// running total, not the size of this match, so folding it in as an
    /// increment inflates the order whenever the feed redelivers.
    fn absorb_event(&mut self, event: FillEvent) {
        let Some(index) = self.resolve(&event.order_id, &event.client_ref, Some(event.at)) else {
            return;
        };
        let record = &mut self.submissions[index];
        if !Self::in_window(record, event.at) {
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
    fn absorb_block(&mut self, block: BlockFill) {
        // The same match, redelivered, is the same match.
        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }

        // Every tracked order the block names. Being named is what the venue
        // closes, so this set is finalised whether or not the division gives an
        // order anything.
        let mut covered: Vec<usize> = Vec::new();
        for order_id in &block.covers {
            if let Some(index) = self.best_by_order_id(order_id, Some(block.at)) {
                if !covered.contains(&index) {
                    covered.push(index);
                }
            }
        }

        let mut keys: Vec<(Timestamp, u64, OrderId, ClientRef, usize)> = Vec::new();
        for &index in &covered {
            let record = &self.submissions[index];
            if record.finalized || !Self::in_window(record, block.at) {
                continue;
            }
            if record.outstanding() <= 0 {
                continue;
            }
            let order_id = match record.order_id.clone() {
                Some(order_id) => order_id,
                None => continue,
            };
            keys.push((
                record.window_start,
                record.venue_seq.unwrap_or(0),
                order_id,
                record.client_ref.clone(),
                index,
            ));
        }

        // The ordering is part of the rule: window start, then venue tick, then
        // the identifier compared lexicographically. `covers` arrives in whatever
        // order the venue built it, and the iteration order of the container
        // holding the records must never decide the outcome, so the eligible set
        // is sorted explicitly before anything is divided.
        keys.sort();

        let basis: Qty = keys
            .iter()
            .map(|key| self.submissions[key.4].outstanding())
            .sum();
        if basis <= 0 {
            self.finalize(&covered);
            return;
        }

        // Quantity above the basis belongs to orders this engine does not track.
        let divisible = if block.block_qty < basis {
            block.block_qty
        } else {
            basis
        };

        let mut shares: Vec<Qty> = Vec::with_capacity(keys.len());
        let mut assigned: Qty = 0;
        for key in &keys {
            let share = proportional_share(divisible, self.submissions[key.4].outstanding(), basis);
            assigned += share;
            shares.push(share);
        }

        // Truncation leaves a few base units unassigned: `0 <= r < |E|`. The
        // first `r` orders in the ordering from the step above take one each, and
        // no other order takes one.
        let mut remainder = divisible - assigned;
        for share in shares.iter_mut() {
            if remainder <= 0 {
                break;
            }
            *share += 1;
            remainder -= 1;
        }

        for (key, share) in keys.iter().zip(shares) {
            self.submissions[key.4].block_filled += share;
        }

        self.finalize(&covered);
    }

    /// A block is final for every order it names, whatever it gave them: the
    /// venue has closed the order and cancelled whatever part of it the block did
    /// not fill. That includes an order the division skipped because its window
    /// had closed or because it had no outstanding quantity left — eligibility
    /// decides what an order is allocated, not whether the venue closed it.
    fn finalize(&mut self, covered: &[usize]) {
        for &index in covered {
            if let Some(record) = self.submissions.get_mut(index) {
                record.finalized = true;
            }
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp: `open_orders` is point-in-time truth
    /// about what is live right now, and an identifier that is live is by
    /// definition still the order the engine tracked, so the window does not
    /// apply to it. It also supplies a `venue_seq` for an order the engine had not
    /// been told one for.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) = self.resolve(&open_order.order_id, &open_order.client_ref, None)
            else {
                continue;
            };
            let record = &mut self.submissions[index];
            if record.venue_seq.is_none() {
                record.venue_seq = Some(open_order.venue_seq);
            }
            if record.finalized {
                continue;
            }
            if open_order.filled_qty > record.direct_filled {
                record.direct_filled = open_order.filled_qty;
            }
        }
    }
}
