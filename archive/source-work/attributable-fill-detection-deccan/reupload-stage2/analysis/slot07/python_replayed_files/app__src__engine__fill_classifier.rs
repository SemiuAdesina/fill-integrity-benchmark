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
//! across the orders it covers. `/app/spec/reconciliation-contract.md` states how
//! the parts accumulate.
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

/// One member of a block's eligible set, carrying the three fields the
/// documented ordering is evaluated over.
struct Eligible {
    window_start: Timestamp,
    venue_seq: u64,
    order_id: OrderId,
    index: usize,
    outstanding: Qty,
}

impl AttributionLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuild a ledger from the fields a `Snapshot` carried across a restart.
    ///
    /// Every field a record carries is restored as it was, including the
    /// *original* window start: a restart that reopened a window would admit an
    /// observation from outside the one the venue actually guarantees.
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
    /// leaves the existing attribution state and window alone.
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

    /// Whether a block has closed this submission at the venue.
    pub fn is_finalized(&self, client_ref: &ClientRef) -> bool {
        self.record(client_ref).map_or(false, |record| record.finalized)
    }

    /// The venue sequence of a submission, once anything has reported one.
    pub fn venue_seq_for(&self, client_ref: &ClientRef) -> Option<u64> {
        self.record(client_ref).and_then(|record| record.venue_seq)
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

    fn index_of_order(&self, order_id: &OrderId) -> Option<usize> {
        self.submissions
            .iter()
            .position(|record| record.order_id.as_ref() == Some(order_id))
    }

    /// Resolve an observation to one of our submissions.
    ///
    /// The `client_ref` the engine chose is authoritative: it is the only key the
    /// engine always knows, and the venue recycles `order_id`s once a window has
    /// closed, so an id alone can name an order that is no longer this one. The
    /// venue id is used only for an observation whose client reference the engine
    /// does not already hold.
    ///
    /// Learning an id this way is what lets an observation reach a submission the
    /// venue never acknowledged.
    fn locate(&mut self, order_id: &OrderId, client_ref: &ClientRef) -> Option<usize> {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            let claimed_elsewhere = self
                .submissions
                .iter()
                .enumerate()
                .any(|(other, record)| other != index && record.order_id.as_ref() == Some(order_id));
            if self.submissions[index].order_id.is_none() && !claimed_elsewhere {
                self.submissions[index].order_id = Some(order_id.clone());
            }
            return Some(index);
        }
        self.index_of_order(order_id)
    }

    /// Whether a venue match timestamp falls in this record's attribution window.
    ///
    /// The window is half-open: `[start, start + ATTRIBUTION_WINDOW)`. An instant
    /// exactly at the start is inside it, and an instant exactly at the end is
    /// outside it.
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
    /// `cumulative_filled_qty` is the order's running total, not the size of this
    /// particular match, and the feed delivers at least once. Folding it in is
    /// therefore raising a high-water mark: a replayed or out-of-order event
    /// moves nothing.
    fn absorb_event(&mut self, event: FillEvent) {
        let Some(index) = self.locate(&event.order_id, &event.client_ref) else {
            return;
        };
        let record = &mut self.submissions[index];
        // Tested against the venue's own match timestamp, not against the cycle
        // that happened to read the event.
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
        // `block_ref` is the idempotency key: the same reference is the same
        // match however many times the feed delivers it.
        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }

        // Eligible: tracked, inside its own window at the block's match time,
        // not already closed by an earlier block, and still short of its size.
        let mut eligible: Vec<Eligible> = Vec::new();
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        for order_id in &block.covers {
            let Some(index) = self.index_of_order(order_id) else {
                continue;
            };
            if !seen.insert(index) {
                continue;
            }
            let record = &self.submissions[index];
            if record.finalized {
                continue;
            }
            if !Self::in_window(record, block.at) {
                continue;
            }
            let outstanding = record.outstanding();
            if outstanding <= 0 {
                continue;
            }
            eligible.push(Eligible {
                window_start: record.window_start,
                venue_seq: record.venue_seq.unwrap_or(0),
                order_id: order_id.clone(),
                index,
                outstanding,
            });
        }

        // `covers` arrives in whatever order the venue happened to build it, so
        // put the eligible set in the documented order before dividing anything:
        // window start, then venue sequence, then order id. All three are
        // evaluated, so the iteration order of the container cannot decide who
        // receives the leftover base units.
        eligible.sort_by(|left, right| {
            (left.window_start, left.venue_seq, &left.order_id).cmp(&(
                right.window_start,
                right.venue_seq,
                &right.order_id,
            ))
        });

        // The basis is current outstanding quantity, not order size: a later
        // block divides over what earlier observations left.
        let basis: Qty = eligible.iter().map(|entry| entry.outstanding).sum();
        if basis <= 0 {
            // Fail closed: no outstanding quantity to divide against. An empty
            // eligible set still closes every tracked order the block named.
            self.finalize(&block.covers);
            return;
        }

        // Quantity above the basis belongs to orders this engine does not track.
        let divisible = if block.block_qty < basis {
            if block.block_qty > 0 {
                block.block_qty
            } else {
                0
            }
        } else {
            basis
        };

        let mut assigned: Qty = 0;
        let mut shares: Vec<Qty> = Vec::with_capacity(eligible.len());
        for entry in &eligible {
            let share = proportional_share(divisible, entry.outstanding, basis);
            assigned += share;
            shares.push(share);
        }

        // Truncation leaves `r` base units, with `0 <= r < |E|`. The first `r`
        // orders in the ordering take one each; no other order takes one.
        let remainder = divisible - assigned;
        for position in 0..(if remainder > 0 { remainder } else { 0 }) as usize {
            if let Some(share) = shares.get_mut(position) {
                *share += 1;
            }
        }

        for (entry, share) in eligible.iter().zip(shares) {
            self.submissions[entry.index].block_filled += share;
        }

        self.finalize(&block.covers);
    }

    /// A block is final for every order it names, whatever it gave them.
    ///
    /// Finalization is a statement about the venue rather than about the
    /// arithmetic: the venue has closed the order and cancelled whatever part of
    /// it the block did not fill. That includes a tracked order that was not
    /// eligible at all — outside its window, or already with nothing outstanding.
    fn finalize(&mut self, covers: &[OrderId]) {
        for order_id in covers {
            if let Some(index) = self.index_of_order(order_id) {
                self.submissions[index].finalized = true;
            }
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp, so no window is tested against them:
    /// `open_orders` is point-in-time truth about what is live right now, and an
    /// identifier that is live is still the order the engine tracked. It also
    /// supplies a `venue_seq` for an order the engine had not been told one for.
    ///
    /// `filled_qty` is the same running total a fill event reports, seen from the
    /// other side, so this is the same high-water mark rather than a second fill.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) = self.locate(&open_order.order_id, &open_order.client_ref) else {
                continue;
            };
            let record = &mut self.submissions[index];
            if record.venue_seq.is_none() {
                record.venue_seq = Some(open_order.venue_seq);
            }
            if open_order.filled_qty > record.direct_filled {
                record.direct_filled = open_order.filled_qty;
            }
        }
    }
}
