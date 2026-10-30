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
//! across the orders it covers. `/app/spec/reconciliation-contract.md` is the
//! authority on how the resulting observations accumulate.
//!
//! The ledger also owns the feed cursor, because the cursor and the state it
//! produced have to be saved and restored together.

use std::collections::{BTreeMap, BTreeSet};

use crate::ports::{BlockFill, FillBatch, FillEvent, OpenOrder};
use crate::types::ids::{proportional_share, BlockRef, ClientRef, OrderId, Qty};
use crate::types::pending_order::ATTRIBUTION_WINDOW;
use crate::types::snapshot::{Cursor, SubmissionRecord};
use crate::types::time::Timestamp;

/// The key an order is sorted by when a block divides.
///
/// Step 2 of the division rule fixes the order of the eligible set: ascending
/// attribution-window start, then ascending `venue_seq`, then ascending
/// `order_id` compared lexicographically. Nothing else may decide the outcome,
/// and in particular the iteration order of the container holding the orders may
/// not. An order the venue never acknowledged has no `venue_seq`; it sorts
/// before every acknowledged one, deterministically.
type BlockOrderKey = (Timestamp, Option<u64>, OrderId);

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
    ///
    /// A restart must not reopen a window: the start each record carries is the
    /// start it was tracked with, not the instant it was read back.
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
    /// leaves the existing attribution state and window start alone.
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

    /// Quantity attributed to one submission so far: its direct progress plus
    /// its block progress.
    pub fn attributed_for(&self, client_ref: &ClientRef) -> Qty {
        self.record(client_ref)
            .map(SubmissionRecord::attributed)
            .unwrap_or(0)
    }

    /// Quantity this submission could still acquire. A submission the venue has
    /// closed can acquire nothing more, whatever it managed to fill, so it is
    /// neither outstanding for a later block's basis nor still working.
    pub fn outstanding_for(&self, client_ref: &ClientRef) -> Qty {
        match self.record(client_ref) {
            Some(record) if record.finalized => 0,
            Some(record) => record.outstanding(),
            None => 0,
        }
    }

    /// Whether the venue has closed this submission. A closed order is not live
    /// and will never acquire another base unit.
    pub fn is_finalized(&self, client_ref: &ClientRef) -> bool {
        self.record(client_ref).map_or(false, |record| record.finalized)
    }

    /// The venue's id for a submission, once anything has reported one.
    pub fn order_for(&self, client_ref: &ClientRef) -> Option<OrderId> {
        self.record(client_ref).and_then(|record| record.order_id.clone())
    }

    fn record(&self, client_ref: &ClientRef) -> Option<&SubmissionRecord> {
        self.index_by_client_ref
            .get(client_ref)
            .and_then(|index| self.submissions.get(*index))
    }

    /// Resolve a venue id to one of our submissions.
    ///
    /// The venue recycles identifiers once a window has closed, so an id alone is
    /// ambiguous: the window is what makes it usable. Prefer a record whose
    /// window contains the instant being tested; failing that, the most recently
    /// tracked record carrying the id, which is the newest order to hold it.
    fn resolve_by_order_id(&self, order_id: &OrderId, at: Option<Timestamp>) -> Option<usize> {
        let mut fallback = None;
        for (index, record) in self.submissions.iter().enumerate() {
            if record.order_id.as_ref() != Some(order_id) {
                continue;
            }
            let in_window = match at {
                Some(instant) => Self::in_window(record, instant),
                None => true,
            };
            if in_window {
                return Some(index);
            }
            if fallback.is_none() {
                fallback = Some(index);
            }
        }
        fallback
    }

    /// Resolve an observation to one of our submissions.
    ///
    /// `client_ref` is the engine's own key and is the only key it always knows,
    /// so it is tried first: a submission whose acknowledgement was lost has no
    /// venue id at all, and a recycled id belongs to a different order than the
    /// one the engine tracked. The venue id is only consulted for an observation
    /// whose client reference this ledger does not know.
    fn locate(&mut self, order_id: &OrderId, client_ref: &ClientRef) -> Option<usize> {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            return Some(index);
        }
        self.resolve_by_order_id(order_id, None)
    }

    /// Whether a venue match timestamp falls in this record's attribution window.
    ///
    /// The window is half-open, `[start, start + ATTRIBUTION_WINDOW)`: an
    /// observation at the start is inside it, one at the end is not.
    fn in_window(record: &SubmissionRecord, at: Timestamp) -> bool {
        at >= record.window_start && at < record.window_start + ATTRIBUTION_WINDOW
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

    /// Fold one per-order fill event in.
    ///
    /// `cumulative_filled_qty` is that order's running total, not the size of
    /// this particular match, and delivery is at least once. Folding it in as an
    /// increment inflates the order whenever the feed replays or delivers out of
    /// order, so direct progress is the **maximum** ever observed.
    ///
    /// The event is tested against the record's own window using the venue's
    /// match timestamp, not the cycle that happened to read it.
    fn absorb_event(&mut self, event: FillEvent) {
        let Some(index) = self.locate(&event.order_id, &event.client_ref) else {
            return;
        };
        if !Self::in_window(&self.submissions[index], event.at) {
            return;
        }
        // An observation inside the window is unambiguous, so it can supply the
        // venue id for a submission whose acknowledgement was lost.
        let record = &mut self.submissions[index];
        if record.order_id.is_none() {
            record.order_id = Some(event.order_id);
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
        // The same match, redelivered, is the same match. `block_ref` is the
        // idempotency key; there is no maximum to fall back on here, because a
        // block share is additive rather than cumulative.
        if !self.absorbed_blocks.insert(block.block_ref) {
            return;
        }

        // Split the covered orders this engine tracks into the eligible set and
        // the rest. `covers` arrives in whatever order the venue happened to
        // build it, and it may repeat an id, so nothing here depends on that.
        let mut covered: Vec<usize> = Vec::new();
        let mut eligible: Vec<(BlockOrderKey, usize, Qty)> = Vec::new();
        for order_id in &block.covers {
            let Some(index) = self.resolve_by_order_id(order_id, Some(block.at)) else {
                continue;
            };
            if covered.contains(&index) {
                continue;
            }
            covered.push(index);

            // Eligibility: tracked, in window, and with something left to fill.
            // A finalized order has no outstanding quantity left, because the
            // venue has already closed it and cancelled the remainder.
            let outstanding = self.outstanding_of(index);
            if outstanding <= 0 {
                continue;
            }
            if !Self::in_window(&self.submissions[index], block.at) {
                continue;
            }
            eligible.push((self.order_key(index), index, outstanding));
        }

        // Put the eligible set in the ordering step 2 prescribes before any
        // quantity is divided, so that neither the order of `covers` nor the
        // iteration order of the container holding the orders can decide anything.
        eligible.sort_by(|left, right| left.0.cmp(&right.0));

        let basis: Qty = eligible.iter().map(|(_, _, outstanding)| *outstanding).sum();
        if basis <= 0 {
            // Fail closed: the block allocated nothing, but it still closed
            // every tracked order it named.
            self.finalize(&covered);
            return;
        }

        // Quantity above the basis belongs to orders this engine does not track.
        let divisible = if block.block_qty < basis {
            block.block_qty
        } else {
            basis
        };
        let divisible = if divisible > 0 { divisible } else { 0 };

        let mut assigned: Qty = 0;
        let mut shares: Vec<Qty> = Vec::with_capacity(eligible.len());
        for (_, _, outstanding) in &eligible {
            let share = proportional_share(divisible, *outstanding, basis);
            assigned += share;
            shares.push(share);
        }

        // Truncation leaves `r = q - sum(shares)` base units, with `0 <= r < |E|`.
        // The first `r` orders in the ordering from step 2 take one each — not
        // the largest order, and not the largest truncated fraction.
        let mut remainder = divisible - assigned;
        for share in shares.iter_mut().take(if remainder > 0 { remainder as usize } else { 0 }) {
            *share += 1;
            remainder -= 1;
        }

        for ((_, index, _), share) in eligible.iter().zip(shares) {
            self.submissions[*index].block_filled += share;
        }

        // A block is final for every order it names, whatever it gave them:
        // the venue closed each of them and cancelled whatever part of it the
        // block did not fill.
        self.finalize(&covered);
    }

    /// The sort key step 2 of the division rule prescribes: attribution-window
    /// start, then `venue_seq`, then `order_id`.
    fn order_key(&self, index: usize) -> BlockOrderKey {
        let record = &self.submissions[index];
        (
            record.window_start,
            record.venue_seq,
            record.order_id.clone().unwrap_or_else(|| OrderId::new("")),
        )
    }

    /// Outstanding quantity of the record at `index`: zero once the venue has
    /// closed it, whatever it managed to fill.
    fn outstanding_of(&self, index: usize) -> Qty {
        let record = &self.submissions[index];
        if record.finalized {
            0
        } else {
            record.outstanding()
        }
    }

    /// A block is final for every order it names, whatever it gave them.
    fn finalize(&mut self, covered: &[usize]) {
        for index in covered {
            self.submissions[*index].finalized = true;
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp: `open_orders` is point-in-time truth
    /// about what is live right now, and an identifier that is live is by
    /// definition still the order the engine tracked, so the attribution window
    /// does not apply to it. `filled_qty` is the same running total a fill event
    /// reports, so it raises the high-water mark rather than adding to it.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) = self.locate(&open_order.order_id, &open_order.client_ref) else {
                continue;
            };
            let record = &mut self.submissions[index];
            if record.order_id.is_none() {
                record.order_id = Some(open_order.order_id.clone());
            }
            if record.venue_seq.is_none() {
                record.venue_seq = Some(open_order.venue_seq);
            }
            if open_order.filled_qty > record.direct_filled {
                record.direct_filled = open_order.filled_qty;
            }
        }
    }
}
