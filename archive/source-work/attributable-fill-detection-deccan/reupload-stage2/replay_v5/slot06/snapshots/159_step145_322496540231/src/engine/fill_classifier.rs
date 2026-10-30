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
//! A record is identified by the `client_ref` the engine chose, because that is
//! the only key always known: the venue's `order_id` is unknown until the venue
//! names it, and an order whose acknowledgement was lost has none. Once the venue
//! does name an `order_id` for a `client_ref`, either key resolves to the same
//! record — but the client reference wins when the two disagree, because the venue
//! recycles identifiers once an attribution window has closed.
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
    /// Every record the venue has been seen to label with a given identifier.
    /// More than one is possible: the venue recycles identifiers, and a record is
    /// only ever dropped once the mandate it belongs to has settled.
    index_by_order_id: BTreeMap<OrderId, Vec<usize>>,
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
            index_by_order_id: BTreeMap::new(),
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
        self.index_by_client_ref.clear();
        self.index_by_order_id.clear();
        for (index, record) in self.submissions.iter().enumerate() {
            self.index_by_client_ref
                .insert(record.client_ref.clone(), index);
            if let Some(order_id) = record.order_id.clone() {
                self.index_by_order_id
                    .entry(order_id)
                    .or_default()
                    .push(index);
            }
        }
    }

    /// Begin tracking an order the engine has just submitted, opening its window.
    ///
    /// Called again for the same `client_ref` — which is what a resubmission
    /// looks like — this fills in whatever the venue has since supplied and
    /// leaves the existing window, and the existing progress, alone.
    pub fn track(
        &mut self,
        client_ref: ClientRef,
        order_id: Option<OrderId>,
        venue_seq: Option<u64>,
        size: Qty,
        window_start: Timestamp,
    ) {
        let index = match self.index_by_client_ref.get(&client_ref).copied() {
            Some(index) => index,
            None => {
                let index = self.submissions.len();
                self.index_by_client_ref.insert(client_ref.clone(), index);
                self.submissions.push(SubmissionRecord {
                    client_ref,
                    order_id: None,
                    window_start,
                    venue_seq: None,
                    size,
                    direct_filled: 0,
                    block_filled: 0,
                    finalized: false,
                });
                index
            }
        };
        if let Some(order_id) = order_id {
            self.note_order_id(index, order_id);
        }
        if venue_seq.is_some() && self.submissions[index].venue_seq.is_none() {
            self.submissions[index].venue_seq = venue_seq;
        }
    }

    /// Reopen the window of an order that has provably never existed at the
    /// venue, so that a later attempt is not judged against a window that never
    /// opened. An order that may exist keeps the window it opened at its first
    /// attempt.
    pub fn reopen_window(&mut self, client_ref: &ClientRef, window_start: Timestamp) {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            self.submissions[index].window_start = window_start;
        }
    }

    /// Attach a venue identifier to a record, once. An identifier is learned
    /// from an acknowledgement, from an observation carrying the client
    /// reference, or from the live order list; it is never revised afterwards,
    /// because the venue hands the same identifier to later orders.
    fn note_order_id(&mut self, index: usize, order_id: OrderId) {
        if self.submissions[index].order_id.is_some() {
            return;
        }
        self.index_by_order_id
            .entry(order_id.clone())
            .or_default()
            .push(index);
        self.submissions[index].order_id = Some(order_id);
    }

    /// Quantity attributed to one submission so far.
    pub fn attributed_for(&self, client_ref: &ClientRef) -> Qty {
        self.record(client_ref)
            .map(SubmissionRecord::attributed)
            .unwrap_or(0)
    }

    /// Whether the venue has closed this submission by naming it in a block.
    pub fn is_finalized(&self, client_ref: &ClientRef) -> bool {
        self.record(client_ref).map(|record| record.finalized).unwrap_or(false)
    }

    /// The venue's id for a submission, once anything has reported one.
    pub fn order_for(&self, client_ref: &ClientRef) -> Option<OrderId> {
        self.record(client_ref).and_then(|record| record.order_id.clone())
    }

    /// Stop tracking a submission. Its mandate has settled, so nothing keyed by
    /// its identifiers may speak for a later order that comes to wear one.
    pub fn forget(&mut self, client_ref: &ClientRef) {
        let Some(index) = self.index_by_client_ref.get(client_ref).copied() else {
            return;
        };
        if let Some(order_id) = self.submissions[index].order_id.take() {
            if let Some(entries) = self.index_by_order_id.get_mut(&order_id) {
                entries.retain(|candidate| *candidate != index);
                if entries.is_empty() {
                    self.index_by_order_id.remove(&order_id);
                }
            }
        }
        self.index_by_client_ref.remove(client_ref);
        self.submissions.remove(index);
        self.reindex();
    }

    fn record(&self, client_ref: &ClientRef) -> Option<&SubmissionRecord> {
        self.index_by_client_ref
            .get(client_ref)
            .and_then(|index| self.submissions.get(*index))
    }

    /// Whether a venue match timestamp falls in this record's attribution
    /// window. The window is half-open: an observation stamped exactly at
    /// `window_start + ATTRIBUTION_WINDOW` names an identifier that is no longer
    /// this order.
    fn in_window(record: &SubmissionRecord, at: Timestamp) -> bool {
        at >= record.window_start && at < record.window_start + ATTRIBUTION_WINDOW
    }

    /// Resolve an identifier to a record, among the records that have been seen
    /// to wear it.
    ///
    /// An identifier is only usable while it is unambiguous, so with a match
    /// timestamp the record whose window contains it wins. Without one — the live
    /// order list carries no timestamp — the most recently opened window wins,
    /// which is the order the venue means now.
    fn lookup_order_id(&self, order_id: &OrderId, at: Option<Timestamp>) -> Option<usize> {
        let candidates = self.index_by_order_id.get(order_id)?;
        let mut best: Option<(Timestamp, ClientRef)> = None;
        let mut best_index: Option<usize> = None;
        for index in candidates {
            let record = &self.submissions[*index];
            if let Some(at) = at {
                if !Self::in_window(record, at) {
                    continue;
                }
            }
            // The client reference is the tie-break rather than the record's
            // position in any container: that position is renumbered whenever an
            // unrelated mandate settles, so it is not a property of the
            // observations and must not decide an attribution outcome.
            let key = (record.window_start, record.client_ref.clone());
            if best.as_ref().map(|best| &key > best).unwrap_or(true) {
                best = Some(key);
                best_index = Some(*index);
            }
        }
        best_index
    }

    /// Resolve one per-order observation to a submission.
    ///
    /// The client reference is tried first: every observation carries one, it is
    /// the key this engine chose, and it is never recycled. The identifier is a
    /// fallback for an observation the engine cannot place by reference, and it
    /// is only honoured inside a window, where it is unambiguous.
    fn locate(&mut self, client_ref: &ClientRef, order_id: &OrderId) -> Option<usize> {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            return Some(index);
        }
        self.lookup_order_id(order_id, None)
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

    /// Direct progress is cumulative, on both sources that report it. A fill
    /// event's `cumulative_filled_qty` and a live entry's `filled_qty` are the
    /// same figure about the same order, so the attributed total is the maximum
    /// ever observed and never a sum.
    fn absorb_event(&mut self, event: FillEvent) {
        let Some(index) = self.locate(&event.client_ref, &event.order_id) else {
            return;
        };
        if !Self::in_window(&self.submissions[index], event.at) {
            return;
        }
        self.note_order_id(index, event.order_id);
        if event.cumulative_filled_qty > self.submissions[index].direct_filled {
            self.submissions[index].direct_filled = event.cumulative_filled_qty;
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

        let mut eligible: Vec<(usize, Timestamp, u64, OrderId, Qty)> = Vec::new();
        for order_id in &block.covers {
            let Some(index) = self.lookup_order_id(order_id, Some(block.at)) else {
                continue;
            };
            if eligible.iter().any(|entry| entry.0 == index) {
                continue;
            }
            let record = &self.submissions[index];
            if !Self::in_window(record, block.at) {
                continue;
            }
            // The venue has already closed it once, so it can acquire nothing
            // more and contributes no outstanding quantity to this basis.
            if record.finalized {
                continue;
            }
            let outstanding = record.outstanding();
            if outstanding <= 0 {
                continue;
            }
            eligible.push((
                index,
                record.window_start,
                record.venue_seq.unwrap_or(0),
                order_id.clone(),
                outstanding,
            ));
        }

        // The documented order, and the only order that may decide anything:
        // window start, then the venue tick that accepted the order, then the
        // identifier. The venue's own order of `covers`, and the iteration order
        // of any container, are both irrelevant to it.
        eligible.sort_by(|left, right| {
            (left.1, left.2, left.3.clone(), left.0)
                .cmp(&(right.1, right.2, right.3.clone(), right.0))
        });

        let basis: Qty = eligible.iter().map(|entry| entry.4).sum();
        if basis <= 0 {
            self.finalize(&block);
            return;
        }

        // Quantity above the basis belongs to orders this engine does not track.
        let divisible = if block.block_qty < basis {
            block.block_qty
        } else {
            basis
        };

        let mut assigned: Qty = 0;
        let mut shares: Vec<Qty> = Vec::with_capacity(eligible.len());
        for entry in &eligible {
            let share = proportional_share(divisible, entry.4, basis);
            assigned += share;
            shares.push(share);
        }

        // Truncation leaves `r` base units unassigned, with `r < |E|`. The first
        // `r` orders in the ordering above take one each. Not the largest, and
        // not the one with the largest truncated fraction.
        let mut remainder = divisible - assigned;
        for share in shares.iter_mut() {
            if remainder <= 0 {
                break;
            }
            *share += 1;
            remainder -= 1;
        }

        for (entry, share) in eligible.iter().zip(shares) {
            self.submissions[entry.0].block_filled += share;
        }

        // Finality is a statement about the venue, not about the arithmetic: it
        // closed every order it named, including any the division skipped.
        self.finalize(&block);
    }

    /// A block closes every order it names, whatever it gave them.
    ///
    /// A covered order whose window had already closed when the block matched,
    /// or that had no outstanding quantity left, is closed just the same. The
    /// window still does one piece of work here: an order opened after the match
    /// cannot be the one the block named, however it came to wear the
    /// identifier.
    fn finalize(&mut self, block: &BlockFill) {
        for order_id in &block.covers {
            let Some(candidates) = self.index_by_order_id.get(order_id).cloned() else {
                continue;
            };
            for index in candidates {
                if self.submissions[index].window_start <= block.at {
                    self.submissions[index].finalized = true;
                }
            }
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp: `open_orders` is point-in-time truth
    /// about what is live right now, and it also supplies a `venue_seq` for an
    /// order the engine had not been told one for. An identifier that is live is
    /// by definition still the order being tracked, so the window does not apply.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) = self.locate(&open_order.client_ref, &open_order.order_id) else {
                continue;
            };
            self.note_order_id(index, open_order.order_id.clone());
            if self.submissions[index].venue_seq.is_none() {
                self.submissions[index].venue_seq = Some(open_order.venue_seq);
            }
            if open_order.filled_qty > self.submissions[index].direct_filled {
                self.submissions[index].direct_filled = open_order.filled_qty;
            }
        }
    }
}
