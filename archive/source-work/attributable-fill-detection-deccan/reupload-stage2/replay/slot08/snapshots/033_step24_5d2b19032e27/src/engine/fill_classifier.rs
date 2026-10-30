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
//! authority on how the two parts accumulate:
//!
//! - direct progress is **cumulative**: a fill event's `cumulative_filled_qty` and
//!   an open order's `filled_qty` both state the running total for that order, so
//!   each one raises a high-water mark rather than adding to a sum;
//! - block progress is **additive**: a derived share is an increment, and
//!   `block_ref` is what stops a redelivery counting it twice.
//!
//! The ledger also owns the feed cursor, because the cursor and the state it
//! produced have to be saved and restored together.

use std::collections::{BTreeMap, BTreeSet};

use crate::ports::{BlockFill, FillBatch, FillEvent, OpenOrder};
use crate::types::ids::{proportional_share, BlockRef, ClientRef, OrderId, Qty};
use crate::types::pending_order::ATTRIBUTION_WINDOW;
use crate::types::snapshot::{Cursor, SubmissionRecord};
use crate::types::time::Timestamp;

/// The order a block is divided in: attribution window start first, then the
/// venue sequence number, then the order id compared lexicographically. Orders
/// with no known sequence number sort after those that have one.
type BlockOrderKey = (Timestamp, u64, OrderId);

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
    /// The window start each record carries is the original one: a restart
    /// reopens nothing.
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
    /// leaves the existing attribution state and the original window start
    /// alone.
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

    /// Attach a venue identity to a submission the engine was already tracking.
    ///
    /// A submission whose acknowledgement was lost has a `client_ref` and no id;
    /// the id arrives later, on an observation carrying that reference.
    pub fn learn_order(&mut self, client_ref: &ClientRef, order_id: OrderId, venue_seq: Option<u64>) {
        let Some(index) = self.index_by_client_ref.get(client_ref).copied() else {
            return;
        };
        let record = &mut self.submissions[index];
        if record.order_id.is_none() {
            record.order_id = Some(order_id);
        }
        if record.venue_seq.is_none() {
            record.venue_seq = venue_seq;
        }
    }

    /// Quantity attributed to one submission so far.
    pub fn attributed_for(&self, client_ref: &ClientRef) -> Qty {
        self.record(client_ref)
            .map(SubmissionRecord::attributed)
            .unwrap_or(0)
    }

    /// Quantity this submission could still acquire.
    ///
    /// A submission the venue has closed — one a block named — can acquire
    /// nothing more, whatever it managed to fill, so it has no outstanding
    /// quantity and contributes nothing to a later block's basis.
    pub fn outstanding_for(&self, client_ref: &ClientRef) -> Qty {
        match self.record(client_ref) {
            Some(record) if record.finalized => 0,
            Some(record) => record.outstanding(),
            None => 0,
        }
    }

    /// Whether the venue has closed this submission.
    pub fn is_finalized(&self, client_ref: &ClientRef) -> bool {
        self.record(client_ref).map(|record| record.finalized).unwrap_or(false)
    }

    pub fn knows(&self, client_ref: &ClientRef) -> bool {
        self.index_by_client_ref.contains_key(client_ref)
    }

    /// The venue's id for a submission, once anything has reported one.
    pub fn order_for(&self, client_ref: &ClientRef) -> Option<OrderId> {
        self.record(client_ref).and_then(|record| record.order_id.clone())
    }

    /// The venue's sequence number for a submission, once anything has reported
    /// one. Shared by every order accepted in the same tick, so it orders
    /// arrivals without identifying an order.
    pub fn venue_seq_for(&self, client_ref: &ClientRef) -> Option<u64> {
        self.record(client_ref).and_then(|record| record.venue_seq)
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
    /// The `client_ref` the engine chose is the primary key: it is the only key
    /// the engine always knows, and it is the one the venue echoes back. The
    /// venue's `order_id` is a fallback for an observation that names an order
    /// this engine tracks under a reference the observation does not carry.
    /// Learning an id this way is what lets a later observation reach a
    /// submission the venue never acknowledged.
    fn locate(&mut self, order_id: &OrderId, client_ref: &ClientRef) -> Option<usize> {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            if self.submissions[index].order_id.is_none() {
                self.submissions[index].order_id = Some(order_id.clone());
            }
            return Some(index);
        }
        self.index_of_order(order_id)
    }

    /// Whether a venue match timestamp falls in this record's attribution
    /// window. The window is half-open: `[start, start + ATTRIBUTION_WINDOW)`.
    /// An instant exactly at the start is inside it; one exactly at the end is
    /// outside it, as is anything later.
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
    /// `cumulative_filled_qty` is the total that order has filled so far, not
    /// the size of this match, so it raises the high-water mark: redelivered,
    /// replayed and out-of-order events must not add quantity.
    fn absorb_event(&mut self, event: FillEvent) {
        let Some(index) = self.locate(&event.order_id, &event.client_ref) else {
            return;
        };
        let record = &mut self.submissions[index];
        // An order the venue has closed will never acquire another base unit.
        if record.finalized {
            return;
        }
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
    /// is integer arithmetic truncated toward zero, the base units left by
    /// truncation go one each to the first orders in the documented ordering,
    /// and any part of the block that no tracked order can account for is
    /// dropped rather than assigned.
    fn absorb_block(&mut self, block: BlockFill) {
        // The same match, redelivered, is the same match.
        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }

        // One order is one order, however many times the venue names it.
        let covers: Vec<OrderId> = {
            let mut seen: BTreeSet<OrderId> = BTreeSet::new();
            block
                .covers
                .into_iter()
                .filter(|order_id| seen.insert(order_id.clone()))
                .collect()
        };

        let mut eligible: Vec<(usize, Qty, BlockOrderKey)> = Vec::new();
        for order_id in &covers {
            let Some(index) = self.index_of_order(order_id) else {
                continue;
            };
            let record = &self.submissions[index];
            // A finalized order is closed by the venue: it contributes no
            // outstanding quantity to this block's basis.
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
            let key = (
                record.window_start,
                record.venue_seq.unwrap_or(u64::MAX),
                record.order_id.clone().unwrap_or_else(|| order_id.clone()),
            );
            eligible.push((index, outstanding, key));
        }

        // `covers` arrives in whatever order the venue happened to build it, and
        // the outcome must not depend on that, so put the eligible set in the
        // documented order before dividing anything.
        eligible.sort_by(|left, right| left.2.cmp(&right.2));

        let basis: Qty = eligible.iter().map(|(_, outstanding, _)| *outstanding).sum();
        if basis <= 0 {
            self.finalize(&covers);
            return;
        }

        // Quantity above the basis belongs to orders this engine does not track.
        let divisible = if block.block_qty < basis {
            block.block_qty
        } else {
            basis
        };

        let mut shares: Vec<Qty> = eligible
            .iter()
            .map(|(_, outstanding, _)| proportional_share(divisible, *outstanding, basis))
            .collect();

        // Truncation leaves base units unassigned. They go one each to the
        // first orders in the ordering above, and to no one else.
        let mut remainder = divisible - shares.iter().sum::<Qty>();
        for share in shares.iter_mut() {
            if remainder <= 0 {
                break;
            }
            *share += 1;
            remainder -= 1;
        }

        for ((index, _, _), share) in eligible.iter().zip(shares) {
            self.submissions[*index].block_filled += share;
        }

        self.finalize(&covers);
    }

    /// A block is final for every order it names, whatever it gave them: the
    /// venue closed the order and cancelled whatever part of it the block did
    /// not fill. That includes an order that was ineligible, and an order the
    /// division allocated nothing to.
    fn finalize(&mut self, covers: &[OrderId]) {
        for order_id in covers {
            if let Some(index) = self.index_of_order(order_id) {
                self.submissions[index].finalized = true;
            }
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp: `open_orders` is point-in-time truth
    /// about what is live right now, so no window is tested against it, and it
    /// also supplies a `venue_seq` for an order the engine had not been told
    /// one for. `filled_qty` is that order's running total, so it raises the
    /// same high-water mark a fill event does.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) = self.locate(&open_order.order_id, &open_order.client_ref) else {
                continue;
            };
            let record = &mut self.submissions[index];
            if record.finalized {
                continue;
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
