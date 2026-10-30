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

/// How an eligible order sorts when a block total is divided.
///
/// The ordering is part of the division rule: the window start first, then the
/// venue sequence, then the order identifier. The iteration order of the
/// container holding the submissions must never decide an outcome, so the
/// eligible set is sorted explicitly before anything is allocated.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BlockRank {
    window_start: Timestamp,
    venue_seq: u64,
    order_id: OrderId,
}

impl Ord for BlockRank {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.window_start
            .cmp(&other.window_start)
            .then(self.venue_seq.cmp(&other.venue_seq))
            .then(self.order_id.cmp(&other.order_id))
    }
}

impl PartialOrd for BlockRank {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

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

    /// Record an order the engine has committed to, opening its window now.
    ///
    /// Called again for the same `client_ref` — which is what a resubmission
    /// under the venue's idempotency key looks like — this fills in whatever the
    /// venue has since supplied, and leaves the original window start and the
    /// attribution accumulated so far alone. A restart must not reopen a window.
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

    /// The venue's id for a submission, once anything has reported one.
    pub fn order_for(&self, client_ref: &ClientRef) -> Option<OrderId> {
        self.record(client_ref).and_then(|record| record.order_id.clone())
    }

    /// Whether the venue has closed this submission.
    ///
    /// A finalized order is neither live nor still working, so it must not be
    /// counted as quantity that will still arrive.
    pub fn is_finalized(&self, client_ref: &ClientRef) -> bool {
        self.record(client_ref).map(|record| record.finalized).unwrap_or(false)
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

    /// Resolve an observation to one of this engine's own submissions.
    ///
    /// The client reference is the primary key. It is the only identifier the
    /// engine always knows, and the venue recycles `order_id` once a window
    /// closes, so an identifier on its own is not safe: an observation about a
    /// recycled identifier names an order that is no longer this one. The venue
    /// id is only a fallback, for an observation that names an order this engine
    /// tracks but does not carry a reference for.
    fn locate(&self, client_ref: &ClientRef, order_id: &OrderId) -> Option<usize> {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            return Some(index);
        }
        self.index_of_order(order_id)
    }

    /// Record the venue's identifier for a submission, once one is known.
    ///
    /// Only an unknown identifier is filled in: an observation about a recycled
    /// identifier must never relabel a submission the venue has already named.
    fn bind_order_id(&mut self, index: usize, order_id: &OrderId) {
        if self.submissions[index].order_id.is_none() {
            self.submissions[index].order_id = Some(order_id.clone());
        }
    }

    /// Whether a venue match timestamp falls in this record's attribution window.
    ///
    /// The window is half-open, `[start, start + ATTRIBUTION_WINDOW)`: the first
    /// instant is inside it and the last is outside it.
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

    fn absorb_event(&mut self, event: FillEvent) {
        let Some(index) = self.locate(&event.client_ref, &event.order_id) else {
            return;
        };
        // The window is tested against the venue's own match timestamp, not
        // against the cycle that happened to read the event: the feed lags, and
        // the same event can arrive several cycles late or be replayed long
        // afterwards.
        if !Self::in_window(&self.submissions[index], event.at) {
            return;
        }
        self.bind_order_id(index, &event.order_id);
        let record = &mut self.submissions[index];
        // `cumulative_filled_qty` is the order's running total, not the size of
        // this match, so a redelivered, replayed or stale event raises a
        // high-water mark and never adds to it.
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
        // idempotency key, so this is the only place a block is counted twice.
        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }

        // Tracked orders this block names, each counted once however `covers`
        // happens to list it.
        let mut covered: Vec<usize> = Vec::new();
        let mut seen: BTreeSet<OrderId> = BTreeSet::new();
        for order_id in &block.covers {
            if !seen.insert(order_id.clone()) {
                continue;
            }
            if let Some(index) = self.index_of_order(order_id) {
                covered.push(index);
            }
        }

        // Eligible set: tracked, the block's match timestamp inside that order's
        // own half-open window, and outstanding quantity still greater than zero.
        let mut eligible: Vec<usize> = covered
            .iter()
            .copied()
            .filter(|&index| {
                let record = &self.submissions[index];
                Self::in_window(record, block.at) && record.outstanding() > 0
            })
            .collect();

        // `covers` arrives in whatever order the venue built it, and the outcome
        // must not depend on that, so rank the eligible set before dividing.
        eligible.sort_by_key(|&index| {
            let record = &self.submissions[index];
            BlockRank {
                window_start: record.window_start,
                venue_seq: record.venue_seq.unwrap_or(0),
                order_id: record
                    .order_id
                    .clone()
                    .unwrap_or_else(|| OrderId::new(String::new())),
            }
        });

        let basis: Qty = eligible
            .iter()
            .map(|&index| self.submissions[index].outstanding())
            .sum();

        // Quantity above the basis belongs to orders this engine does not track,
        // so it is not attributable to anything and is dropped.
        let q = if block.block_qty < basis {
            block.block_qty
        } else {
            basis
        };

        if basis <= 0 || q <= 0 {
            // The fail-closed case: a block whose covered orders can take
            // nothing is evidence about some other order, not about these. It
            // still closes every order it names.
            self.finalize(&covered);
            return;
        }

        let mut shares: Vec<Qty> = eligible
            .iter()
            .map(|&index| {
                let outstanding = self.submissions[index].outstanding();
                proportional_share(q, outstanding, basis)
            })
            .collect();

        // Truncation leaves `0 <= r < |E|` base units unplaced. The first `r`
        // orders in the ranking take one each; no other order takes one, and
        // nothing goes to the largest order or the largest truncated fraction.
        let mut remainder = q - shares.iter().sum::<Qty>();
        for share in shares.iter_mut() {
            if remainder <= 0 {
                break;
            }
            *share += 1;
            remainder -= 1;
        }

        for (&index, share) in eligible.iter().zip(shares) {
            self.submissions[index].block_filled += share;
        }

        self.finalize(&covered);
    }

    /// A block is final for every order it names, whatever it gave them.
    ///
    /// Finalization is a statement about the venue rather than about the
    /// arithmetic: the venue closed the order and cancelled whatever part of it
    /// the block did not fill. That includes an order the division allocated
    /// nothing to — one outside its window, or one with nothing outstanding —
    /// because eligibility decides what an order is allocated, not whether the
    /// venue closed it.
    fn finalize(&mut self, covered: &[usize]) {
        for &index in covered {
            self.submissions[index].finalized = true;
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp: `open_orders` is point-in-time truth
    /// about what is live right now, and an identifier that is live is still the
    /// order this engine tracked, so the window does not apply to it. The list
    /// also supplies a `venue_seq` for an order the engine had not been told one
    /// for.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) = self.locate(&open_order.client_ref, &open_order.order_id) else {
                continue;
            };
            self.bind_order_id(index, &open_order.order_id);
            let record = &mut self.submissions[index];
            if record.venue_seq.is_none() {
                record.venue_seq = Some(open_order.venue_seq);
            }
            // `filled_qty` is the total this order has filled so far, the very
            // quantity a fill event reports as `cumulative_filled_qty`. The two
            // raise one high-water mark between them; adding one source on top
            // of the other double counts the same base units.
            if open_order.filled_qty > record.direct_filled {
                record.direct_filled = open_order.filled_qty;
            }
        }
    }
}
