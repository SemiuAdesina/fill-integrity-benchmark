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
//! authority on how the parts accumulate.
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

    /// Record an order the engine has just committed to, opening its window now.
    ///
    /// The window opens at the instant the engine starts tracking the order, so
    /// it is opened here rather than when the venue acknowledges: an order whose
    /// acknowledgement was lost may already exist at the venue from this instant.
    ///
    /// Called again for the same `client_ref` — which is what a resubmission
    /// under the engine's idempotency key looks like — this fills in whatever the
    /// venue has since supplied and leaves the existing window start and
    /// attribution state alone. A restart must not reopen a window: the original
    /// start is what an out-of-window observation has to be tested against.
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

    /// As `outstanding_for`, for a submission the engine knows it committed to
    /// and whose size it remembers itself. A submission the ledger has no record
    /// of has not been closed by anything, so it is still working in full.
    pub fn outstanding_or(&self, client_ref: &ClientRef, fallback_size: Qty) -> Qty {
        match self.record(client_ref) {
            Some(record) if record.finalized => 0,
            Some(record) => record.outstanding(),
            None => fallback_size,
        }
    }

    /// Whether the venue has closed this submission. A closed order is not live
    /// and is not still working, so whatever gap it leaves is real.
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

    fn index_of_order(&self, order_id: &OrderId) -> Option<usize> {
        self.submissions
            .iter()
            .position(|record| record.order_id.as_ref() == Some(order_id))
    }

    /// Resolve an observation to one of our submissions.
    ///
    /// The `client_ref` is the engine's own key and the only one it always
    /// knows, so it is tried first; the venue's `order_id` resolves the same
    /// record once the venue has named it. Learning an id this way is what lets
    /// an observation reach a submission the venue never acknowledged.
    ///
    /// An id is only adopted for a record when no other record already holds
    /// it, so one venue identifier never ends up naming two of our orders.
    fn locate(&mut self, order_id: &OrderId, client_ref: &ClientRef) -> Option<usize> {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            let unclaimed = self.submissions[index].order_id.is_none()
                && self.index_of_order(order_id).is_none();
            if unclaimed {
                self.submissions[index].order_id = Some(order_id.clone());
            }
            return Some(index);
        }
        self.index_of_order(order_id)
    }

    /// Whether a venue match timestamp falls in this record's attribution window.
    ///
    /// The window is half-open: `[start, start + ATTRIBUTION_WINDOW)`. An
    /// observation at the start is inside it, one exactly at the end is outside,
    /// because by then the venue may have recycled the identifier onto an
    /// unrelated order and counting it would be a misattribution.
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
    /// `cumulative_filled_qty` is the running total the venue has matched for
    /// this order, not the size of this particular match, and the feed delivers
    /// at least once. Direct progress is therefore a high-water mark over
    /// everything ever observed for the order: folding the figure in as an
    /// increment inflates the order every time the feed replays.
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
    /// The venue reports only a total and names the orders it was matched
    /// against; the division rule is in `/app/spec/venue-semantics.md`. Every step
    /// is integer arithmetic, and any part of the block that no tracked order can
    /// account for is dropped rather than assigned.
    fn absorb_block(&mut self, block: BlockFill) {
        // The same match, redelivered, is the same match. `block_ref` is the
        // idempotency key, so however often the feed delivers it the block
        // contributes exactly once.
        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }

        // Step 1: the eligible set. Tracked, ordered by the block's own match
        // timestamp under the half-open window rule, not yet closed by the
        // venue, and still short of its size. An order's outstanding quantity is
        // its size minus what has already been attributed to it.
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
                index,
                outstanding,
                window_start: record.window_start,
                venue_seq: record.venue_seq,
                order_id: order_id.clone(),
            });
        }

        // Step 2: order the eligible set deterministically, ascending by window
        // start, then venue sequence, then order id. `covers` arrives in
        // whatever order the venue happened to build it, and no container's
        // iteration order may decide the outcome.
        eligible.sort();

        // Step 3: the basis is the sum of the outstanding quantities in the set.
        let basis: Qty = eligible.iter().map(|entry| entry.outstanding).sum();
        if basis <= 0 {
            // No eligible order, so nothing is allocated. The block still
            // closes every tracked order it names.
            self.finalize(&block.covers);
            return;
        }

        // Step 4: quantity above the basis is not attributable to anything this
        // engine is tracking, and is dropped.
        let divisible = if block.block_qty < basis {
            block.block_qty
        } else {
            basis
        };

        // Step 5: base shares, `(q * outstanding) / basis` truncated toward zero.
        let mut assigned: Qty = 0;
        let mut shares: Vec<Qty> = Vec::with_capacity(eligible.len());
        for entry in &eligible {
            let share = proportional_share(divisible, entry.outstanding, basis);
            assigned += share;
            shares.push(share);
        }

        // Step 6: truncation leaves `r` base units, with `0 <= r < |E|`. The
        // first `r` orders in the ordering from step 2 take one extra unit each,
        // not the largest order and not the largest truncated fraction.
        let mut remainder = divisible - assigned;
        if remainder > 0 {
            for share in shares.iter_mut() {
                if remainder <= 0 {
                    break;
                }
                *share += QTY_UNIT;
                remainder -= QTY_UNIT;
            }
        }

        for (entry, share) in eligible.iter().zip(shares) {
            self.submissions[entry.index].block_filled += share;
        }

        // Every tracked order the block names is closed by the venue, including
        // one it allocated nothing to — one that was ineligible because the
        // match fell outside its window, or because it had no outstanding
        // quantity left. Eligibility decides what an order is allocated; it does
        // not decide whether the venue closed it.
        self.finalize(&block.covers);
    }

    /// A block is final for every order it names, whatever it gave them.
    fn finalize(&mut self, covers: &[OrderId]) {
        for order_id in covers {
            if let Some(index) = self.index_of_order(order_id) {
                self.submissions[index].finalized = true;
            }
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp and none is needed: `open_orders` is
    /// point-in-time truth about what is live right now, and an identifier that
    /// is live is by definition still the order the engine tracked, so the
    /// attribution window does not apply to it.
    ///
    /// `filled_qty` is that order's running total, the same quantity a fill
    /// event reports. It is not a second fill to add on top of one, so it raises
    /// the order's high-water mark and never lowers it.
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

/// One member of a block's eligible set, ordered by the rule the specification
/// states: window start, then venue sequence, then order id.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Eligible {
    index: usize,
    outstanding: Qty,
    window_start: Timestamp,
    venue_seq: Option<u64>,
    order_id: OrderId,
}

impl Ord for Eligible {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.window_start
            .cmp(&other.window_start)
            .then_with(|| self.venue_seq.cmp(&other.venue_seq))
            .then_with(|| self.order_id.cmp(&other.order_id))
            // Covered orders are distinct, so this never decides anything; it
            // is here only to keep the ordering total and consistent with `Eq`.
            .then_with(|| self.index.cmp(&other.index))
    }
}

impl PartialOrd for Eligible {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
