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
//! Two rules shape everything here. A fill event's `cumulative_filled_qty` and an
//! open order's `filled_qty` are one figure seen from two sides — the total that
//! order has filled so far — so direct progress is the **maximum** ever observed
//! and never a sum. A block share is an increment, so block progress adds, and
//! redelivery is stopped by `block_ref` idempotency rather than by a maximum.
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
    /// Called again for the same `client_ref` — which is what a retry after a lost
    /// acknowledgement looks like — this fills in whatever the venue has since
    /// supplied and leaves the window start and the attribution state alone. The
    /// window of a retried order is the one that opened at the first attempt.
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

    /// Forget a submission entirely. Used when a mandate settles: nothing keyed by
    /// its identifiers may influence a later order that comes to wear one of them,
    /// because the venue recycles identifiers once a window has closed.
    pub fn forget(&mut self, client_refs: &BTreeSet<ClientRef>) {
        if client_refs.is_empty() {
            return;
        }
        self.submissions
            .retain(|record| !client_refs.contains(&record.client_ref));
        self.reindex();
    }

    /// Quantity attributed to one submission so far.
    pub fn attributed_for(&self, client_ref: &ClientRef) -> Qty {
        self.record(client_ref)
            .map(SubmissionRecord::attributed)
            .unwrap_or(0)
    }

    /// Quantity this submission could still acquire. A submission the venue has
    /// closed can acquire nothing more, whatever it managed to fill.
    pub fn working_for(&self, client_ref: &ClientRef) -> Qty {
        self.record(client_ref)
            .map(SubmissionRecord::working)
            .unwrap_or(0)
    }

    /// Whether a block has closed this submission at the venue.
    pub fn is_finalized(&self, client_ref: &ClientRef) -> bool {
        self.record(client_ref)
            .map(|record| record.finalized)
            .unwrap_or(false)
    }

    /// Whether this submission is tracked at all.
    pub fn is_tracked(&self, client_ref: &ClientRef) -> bool {
        self.index_by_client_ref.contains_key(client_ref)
    }

    /// The venue's id for a submission, once anything has reported one.
    pub fn order_for(&self, client_ref: &ClientRef) -> Option<OrderId> {
        self.record(client_ref)
            .and_then(|record| record.order_id.clone())
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

    /// Locate the submission that currently holds a venue identifier.
    ///
    /// Identifiers are recycled, so at most one live record may carry one: the
    /// engine drops a record when the mandate that owned it settles, and until then
    /// the identifier is unambiguous inside the window.
    fn index_of_order(&self, order_id: &OrderId) -> Option<usize> {
        self.submissions
            .iter()
            .position(|record| record.order_id.as_ref() == Some(order_id))
    }

    /// Resolve an observation to one of our submissions.
    ///
    /// The `client_ref` is preferred, because it is the engine's own key and it is
    /// never recycled: it names exactly the order the engine chose it for, even one
    /// the venue never acknowledged. The venue's `order_id` is only a fallback for
    /// an observation that carries no reference the engine recognises, and learning
    /// an id this way is what lets a fill reach an order whose acknowledgement was
    /// lost.
    fn locate(&mut self, order_id: &OrderId, client_ref: &ClientRef) -> Option<usize> {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            // The reference is ours, so this observation is about that submission
            // however the venue happens to be labelling it. Adopt the label if the
            // record has none, and correct it if the venue has relabelled the order.
            if self.submissions[index].order_id.as_ref() != Some(order_id) {
                // Whatever else was carrying this label was an earlier order; the
                // identifier now speaks for this one, so strip it from the other.
                for (other, record) in self.submissions.iter_mut().enumerate() {
                    if other != index && record.order_id.as_ref() == Some(order_id) {
                        record.order_id = None;
                    }
                }
                self.submissions[index].order_id = Some(order_id.clone());
            }
            return Some(index);
        }
        self.index_of_order(order_id)
    }

    /// Whether a venue match timestamp falls in this record's attribution window.
    ///
    /// The window is half-open: `[start, start + ATTRIBUTION_WINDOW)`. An
    /// observation stamped exactly at the end names an identifier the venue is free
    /// to have handed to an unrelated order, so counting it is a misattribution.
    fn in_window(record: &SubmissionRecord, at: Timestamp) -> bool {
        at >= record.window_start && at < record.window_start + ATTRIBUTION_WINDOW
    }

    /// Fold one polled batch into the ledger and advance the cursor.
    ///
    /// Returns the number of per-order events in the batch, which the engine
    /// reports through `Engine::observed_fill_events`.
    pub fn absorb_batch(&mut self, batch: FillBatch) -> usize {
        let count = batch.events.len();
        // Blocks and events can arrive in one batch. Sequence numbers are shared
        // across both feeds, so fold them in sequence order: a block finalizes the
        // orders it names, and an event stamped after it must not reopen one.
        let mut events = batch.events;
        let mut blocks = batch.blocks;
        events.sort_by_key(|event| event.seq);
        blocks.sort_by_key(|block| block.seq);
        let mut event_iter = events.into_iter().peekable();
        let mut block_iter = blocks.into_iter().peekable();
        loop {
            let take_event = match (event_iter.peek(), block_iter.peek()) {
                (Some(event), Some(block)) => event.seq <= block.seq,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            if take_event {
                if let Some(event) = event_iter.next() {
                    self.absorb_event(event);
                }
            } else if let Some(block) = block_iter.next() {
                self.absorb_block(block);
            }
        }
        self.cursor = batch.next_cursor;
        count
    }

    /// Fold one per-order fill event into the ledger.
    ///
    /// `cumulative_filled_qty` is that order's running total, not the size of this
    /// match, so it is folded in as a maximum. Adding it would inflate the order
    /// every time the at-least-once feed redelivers.
    fn absorb_event(&mut self, event: FillEvent) {
        let Some(index) = self.locate(&event.order_id, &event.client_ref) else {
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
    /// The venue reports only a total and names the orders it was matched against;
    /// the division rule is in `/app/spec/venue-semantics.md`. Every step is
    /// integer arithmetic, and any part of the block that no tracked order can
    /// account for is dropped rather than assigned.
    fn absorb_block(&mut self, block: BlockFill) {
        // The same match, redelivered, is the same match.
        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }

        // The eligible set: covered orders the engine tracks, whose window
        // contains the block's match instant, with outstanding quantity left.
        let mut eligible: Vec<usize> = Vec::new();
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        for order_id in &block.covers {
            let Some(index) = self.index_of_order(order_id) else {
                continue;
            };
            if !seen.insert(index) {
                continue;
            }
            let record = &self.submissions[index];
            if !Self::in_window(record, block.at) {
                continue;
            }
            // An order a block has already closed contributes no outstanding
            // quantity to a later block's basis, so it is not eligible either.
            if record.working() <= 0 {
                continue;
            }
            eligible.push(index);
        }

        // `covers` arrives in whatever order the venue happened to build it, and no
        // container's iteration order may decide the outcome, so impose the
        // documented ordering: window start, then venue tick, then identifier.
        eligible.sort_by(|left, right| {
            let a = &self.submissions[*left];
            let b = &self.submissions[*right];
            a.window_start
                .cmp(&b.window_start)
                .then_with(|| a.venue_seq.cmp(&b.venue_seq))
                .then_with(|| a.order_id.cmp(&b.order_id))
        });

        let basis: Qty = eligible
            .iter()
            .map(|index| self.submissions[*index].working())
            .sum();

        if basis > 0 {
            // Quantity above the basis belongs to orders this engine does not track.
            let divisible = if block.block_qty < basis {
                block.block_qty
            } else {
                basis
            };

            let mut assigned: Qty = 0;
            let mut shares: Vec<Qty> = Vec::with_capacity(eligible.len());
            for index in &eligible {
                let outstanding = self.submissions[*index].working();
                let share = proportional_share(divisible, outstanding, basis);
                assigned += share;
                shares.push(share);
            }

            // Truncation leaves `0 <= r < |E|` base units unassigned. They go one
            // each to the first `r` orders of the documented ordering — not to the
            // largest order, and not to the largest truncated fraction.
            let mut remainder = divisible - assigned;
            for share in shares.iter_mut() {
                if remainder <= 0 {
                    break;
                }
                *share += 1;
                remainder -= 1;
            }

            for (index, share) in eligible.iter().zip(shares) {
                self.submissions[*index].block_filled += share;
            }
        }

        // A block is final for every order it names — every one, not only the ones
        // the division allocated something to. The venue has closed them all and
        // cancelled whatever part it did not fill.
        self.finalize(&block.covers);
    }

    fn finalize(&mut self, covers: &[OrderId]) {
        for order_id in covers {
            if let Some(index) = self.index_of_order(order_id) {
                self.submissions[index].finalized = true;
            }
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp: `open_orders` is point-in-time truth about
    /// what is live right now, so an identifier that is live is still the order the
    /// engine tracked and the window does not apply to it. `filled_qty` is the same
    /// cumulative figure a fill event reports, so it folds in as a maximum too, and
    /// never on top of it.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) = self.locate(&open_order.order_id, &open_order.client_ref) else {
                continue;
            };
            let record = &mut self.submissions[index];
            record.venue_seq = Some(open_order.venue_seq);
            if open_order.filled_qty > record.direct_filled {
                record.direct_filled = open_order.filled_qty;
            }
        }
    }
}
