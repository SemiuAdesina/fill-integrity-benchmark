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
//! authority on how the three combine.
//!
//! The ledger also owns the feed cursor, because the cursor and the state it
//! produced have to be saved and restored together.

use std::collections::{BTreeMap, BTreeSet};

use crate::ports::{BlockFill, FillBatch, FillEvent, OpenOrder};
use crate::types::ids::{proportional_share, BlockRef, ClientRef, OrderId, Qty, QTY_UNIT};
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
    /// leaves the existing attribution state and window start alone: the window
    /// opens once, at the first instant the engine tracked the order, and a
    /// restart or a retry must not reopen it.
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

    /// Drop a submission the venue never created, so that its `client_ref` can
    /// never again be mistaken for an order.
    fn forget(&mut self, client_ref: &ClientRef) {
        let Some(index) = self.index_by_client_ref.get(client_ref).copied() else {
            return;
        };
        self.submissions.remove(index);
        self.reindex();
    }

    /// Drop a submission the venue never created. Public so the engine can undo a
    /// commitment that turned out to create nothing.
    pub fn discard(&mut self, client_ref: &ClientRef) {
        self.forget(client_ref);
    }

    /// Quantity attributed to one submission so far: its direct progress plus
    /// its block progress.
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

    /// Whether a block naming this order has been observed. A finalized order is
    /// closed by the venue: it is not live and not still working.
    pub fn is_finalized(&self, client_ref: &ClientRef) -> bool {
        self.record(client_ref).map(|record| record.finalized).unwrap_or(false)
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

    /// Every tracked submission carrying this venue id. The venue recycles
    /// identifiers, so more than one is possible in principle; closing a block
    /// over all of them is the safe reading.
    fn indices_of_order(&self, order_id: &OrderId) -> Vec<usize> {
        self.submissions
            .iter()
            .enumerate()
            .filter(|(_, record)| record.order_id.as_ref() == Some(order_id))
            .map(|(index, _)| index)
            .collect()
    }

    fn index_of_order(&self, order_id: &OrderId) -> Option<usize> {
        self.indices_of_order(order_id).into_iter().next()
    }

    /// Resolve an observation to one of our submissions.
    ///
    /// The `client_ref` the engine chose is tried first: it is the only key the
    /// engine always knows, it is never recycled, and it is the key a
    /// resubmission is made under. The venue's `order_id` is the fallback, and
    /// resolving by it is what lets an observation reach a submission the venue
    /// never acknowledged — once the venue names an id for a `client_ref`,
    /// either key denotes the same order.
    fn locate(&mut self, order_id: &OrderId, client_ref: &ClientRef) -> Option<usize> {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            let record = &mut self.submissions[index];
            if record.order_id.is_none() {
                record.order_id = Some(order_id.clone());
            }
            return Some(index);
        }
        self.index_of_order(order_id)
    }

    /// Whether a venue match timestamp falls in this record's attribution window.
    ///
    /// The window is half-open: `[start, start + ATTRIBUTION_WINDOW)`. An
    /// observation exactly at the start is inside it; one exactly at the end is
    /// outside it, and so is anything later.
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
    /// `cumulative_filled_qty` is the total that order has filled so far, not an
    /// increment, so it raises a high-water mark rather than adding: the feed is
    /// at-least-once and may replay an event several times or deliver it several
    /// cycles late.
    fn absorb_event(&mut self, event: FillEvent) {
        let Some(index) = self.locate(&event.order_id, &event.client_ref) else {
            return;
        };
        let record = &self.submissions[index];
        // A block has closed this order: it will never acquire another base
        // unit, so nothing the feed says about it afterwards is new.
        if record.finalized {
            return;
        }
        if !Self::in_window(record, event.at) {
            return;
        }
        let cumulative = event.cumulative_filled_qty;
        let record = &mut self.submissions[index];
        if cumulative > record.direct_filled {
            record.direct_filled = cumulative;
        }
    }

    /// Divide one block match across the orders it covers.
    ///
    /// The venue reports only a total and names the orders it was matched
    /// against; the division rule is in `/app/spec/venue-semantics.md`. Every step
    /// is integer arithmetic, and any part of the block that no tracked order can
    /// account for is dropped rather than assigned.
    fn absorb_block(&mut self, block: BlockFill) {
        // `block_ref` is the idempotency key: the same block_ref is the same
        // match however many times the feed delivers it.
        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }

        // Eligible set: tracked, not already closed by an earlier block, inside
        // its own attribution window at the block's match time, and still short.
        let mut seen: BTreeSet<OrderId> = BTreeSet::new();
        let mut eligible: Vec<(usize, Timestamp, u64, OrderId, Qty)> = Vec::new();
        for order_id in &block.covers {
            if !seen.insert(order_id.clone()) {
                continue;
            }
            let Some(index) = self.index_of_order(order_id) else {
                continue;
            };
            let record = &self.submissions[index];
            if record.finalized {
                continue;
            }
            if !Self::in_window(record, block.at) {
                continue;
            }
            let outstanding = self.outstanding_for(&record.client_ref);
            if outstanding <= 0 {
                continue;
            }
            eligible.push((
                index,
                record.window_start,
                record.venue_seq.unwrap_or(u64::MAX),
                order_id.clone(),
                outstanding,
            ));
        }

        // The documented ordering, coarsest key first: window start, then
        // venue sequence, then order id. `covers` arrives in whatever order the
        // venue built it and the outcome must not depend on that.
        eligible.sort_by(|left, right| {
            (left.1, left.2, left.3.clone()).cmp(&(right.1, right.2, right.3.clone()))
        });

        let basis: Qty = eligible.iter().map(|entry| entry.4).sum();
        let total = if block.block_qty > 0 {
            block.block_qty
        } else {
            0
        };
        if basis <= 0 || total <= 0 {
            // Fail closed: nothing here is attributable to a tracked order. The
            // block still closes every order it names.
            self.finalize(&block.covers);
            return;
        }

        // Quantity above the basis belongs to orders this engine does not track.
        let divisible = if total < basis { total } else { basis };

        let mut shares: Vec<Qty> = eligible
            .iter()
            .map(|entry| proportional_share(divisible, entry.4, basis))
            .collect();

        // Truncation leaves a few base units unassigned. One each to the first
        // orders in the ordering above, and to no other order.
        let mut remainder = divisible - shares.iter().sum::<Qty>();
        let mut position = 0usize;
        while remainder > 0 && position < shares.len() {
            shares[position] += QTY_UNIT;
            remainder -= QTY_UNIT;
            position += 1;
        }

        for (entry, share) in eligible.iter().zip(shares) {
            self.submissions[entry.0].block_filled += share;
        }

        self.finalize(&block.covers);
    }

    /// A block is final for every order it names, whatever it gave them.
    ///
    /// Eligibility decides what an order is allocated; it does not decide whether
    /// the venue closed it. An order the division skipped — because the block's
    /// match time fell outside its window, or because it had no outstanding
    /// quantity left — is closed just the same, as is one the truncated division
    /// gave nothing to.
    fn finalize(&mut self, covers: &[OrderId]) {
        for order_id in covers {
            for index in self.indices_of_order(order_id) {
                self.submissions[index].finalized = true;
            }
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp: `open_orders` is point-in-time truth
    /// about what is live right now, and an identifier that is live is by
    /// definition still the order the engine tracked, so the attribution window
    /// does not apply to it. `filled_qty` is the same running total a fill event
    /// carries, seen from the other side, so it raises the same high-water mark
    /// rather than adding to it.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) = self.locate(&open_order.order_id, &open_order.client_ref) else {
                continue;
            };
            let record = &self.submissions[index];
            if record.finalized {
                continue;
            }
            if record.venue_seq.is_none() {
                self.submissions[index].venue_seq = Some(open_order.venue_seq);
            }
            if open_order.filled_qty > self.submissions[index].direct_filled {
                self.submissions[index].direct_filled = open_order.filled_qty;
            }
        }
    }
}
