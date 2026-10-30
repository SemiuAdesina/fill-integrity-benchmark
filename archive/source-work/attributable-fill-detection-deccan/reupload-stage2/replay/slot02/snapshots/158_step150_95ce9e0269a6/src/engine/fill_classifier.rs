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
//! authority on how the three are combined.
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

/// One covered order a block may allocate a share to, with the keys the
/// documented ordering sorts on.
struct Claimant {
    index: usize,
    outstanding: Qty,
    window_start: Timestamp,
    venue_seq: Option<u64>,
    order_id: OrderId,
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
    /// Called again for the same `client_ref` — which is what a resubmission, a
    /// retry or a later acknowledgement looks like — this fills in whatever the
    /// venue has since supplied and leaves the attribution state, the size and
    /// the *original* window start alone. Reopening the window here would admit
    /// an observation from outside the window the venue actually granted.
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

    /// Whether the venue has closed this submission, so that it will never
    /// acquire another base unit.
    pub fn is_finalized(&self, client_ref: &ClientRef) -> bool {
        self.record(client_ref)
            .map(|record| record.finalized)
            .unwrap_or(false)
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

    /// Every submission the venue has labelled `order_id`.
    ///
    /// The venue recycles identifiers once a window has closed, so more than one
    /// record can carry the same label over a long enough run. Callers that have
    /// to pick one pick the most recently tracked, because a recycled label
    /// belongs to the newer order.
    fn indices_of_order(&self, order_id: &OrderId) -> Vec<usize> {
        self.submissions
            .iter()
            .enumerate()
            .filter(|(_, record)| record.order_id.as_ref() == Some(order_id))
            .map(|(index, _)| index)
            .collect()
    }

    /// Resolve an observation to one of our submissions.
    ///
    /// The `client_ref` the engine chose is the primary key: it is the key the
    /// engine always knows, and it is the only key that stays unambiguous when
    /// the venue recycles an `order_id`. The venue's label is the fallback, and
    /// it is how an observation reaches a submission the venue never
    /// acknowledged — learning the id this way is what resolves a lost
    /// acknowledgement later.
    ///
    /// A timestamped observation only resolves by `order_id` to a submission
    /// whose own attribution window contains it. Outside that window the label
    /// names some other order, and counting the observation is a
    /// misattribution rather than a rounding matter.
    ///
    /// For the same reason an out-of-window observation may not teach the ledger
    /// the venue's label. The quantity it carries is dropped on the window test,
    /// but the label would stick: a later block naming that recycled identifier
    /// would then finalize this submission, or allocate it base units, on the
    /// strength of a replay. Only an observation that is actually about the
    /// order — one carrying no timestamp, or one whose `at` the order's own
    /// window contains — may bind the label to it. An entry of `open_orders`
    /// carries no timestamp and is live now, so it always may.
    fn resolve(
        &mut self,
        client_ref: &ClientRef,
        order_id: &OrderId,
        at: Option<Timestamp>,
    ) -> Option<usize> {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            let attributable =
                at.map_or(true, |at| Self::in_window(&self.submissions[index], at));
            if attributable && self.submissions[index].order_id.is_none() {
                self.submissions[index].order_id = Some(order_id.clone());
            }
            return Some(index);
        }
        self.locate_by_order(order_id, at)
    }

    /// The submission a venue label most plausibly names right now.
    fn locate_by_order(&self, order_id: &OrderId, at: Option<Timestamp>) -> Option<usize> {
        let mut candidates = self.indices_of_order(order_id);
        if let Some(at) = at {
            candidates.retain(|index| Self::in_window(&self.submissions[*index], at));
        }
        candidates.into_iter().max_by_key(|index| {
            let record = &self.submissions[*index];
            (record.window_start, *index)
        })
    }

    /// Whether a venue match timestamp falls in this record's attribution window.
    ///
    /// The window is half-open, `[start, start + ATTRIBUTION_WINDOW)`: an
    /// observation exactly at the start belongs to the order, one exactly at the
    /// end does not. It is the venue's own match timestamp that is tested, never
    /// the cycle in which the engine happened to read the observation.
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
    /// the size of this particular match, and the feed may redeliver it, replay
    /// it long afterwards or deliver it out of order. It is therefore a
    /// high-water mark: repeated, replayed or stale readings move the mark and
    /// never add quantity.
    ///
    /// A block that named the order has already closed it, and a closed order
    /// never acquires another base unit, so finalization freezes the mark where
    /// the block left it. This ledger folds events before blocks within one
    /// batch, so a fill the venue genuinely reported before its block still
    /// counts.
    fn absorb_event(&mut self, event: FillEvent) {
        let Some(index) = self.resolve(&event.client_ref, &event.order_id, Some(event.at)) else {
            return;
        };
        let record = &mut self.submissions[index];
        if record.finalized || !Self::in_window(record, event.at) {
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
    ///
    /// The block also finalizes every tracked order it names, whatever the
    /// division gave them: the venue has closed them all.
    fn absorb_block(&mut self, block: BlockFill) {
        // The same match, redelivered, is the same match.
        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }

        let eligible = self.eligible_set(&block);

        let basis: Qty = eligible.iter().map(|claimant| claimant.outstanding).sum();
        if basis > 0 {
            // Quantity above the basis belongs to orders this engine does not
            // track, so it is dropped rather than invented somewhere.
            let divisible = if block.block_qty < basis {
                block.block_qty
            } else {
                basis
            };

            let mut shares: Vec<Qty> = Vec::with_capacity(eligible.len());
            let mut assigned: Qty = 0;
            for claimant in &eligible {
                let share = proportional_share(divisible, claimant.outstanding, basis);
                assigned += share;
                shares.push(share);
            }

            // Truncation leaves `r` base units, `0 <= r < |E|`. The first `r`
            // orders in the documented ordering take one each; no other order
            // takes one.
            let remainder = divisible - assigned;
            for (position, claimant) in eligible.iter().enumerate() {
                let extra = if remainder > 0 && (position as Qty) < remainder {
                    1
                } else {
                    0
                };
                self.submissions[claimant.index].block_filled += shares[position] + extra;
            }
        }

        self.finalize(&block.covers);
    }

    /// The orders the block may allocate to: tracked, inside their own window at
    /// the block's match timestamp, and still carrying outstanding quantity.
    ///
    /// `covers` arrives in whatever order the venue happened to build it, and
    /// the outcome must not depend on that, so the set is sorted here by
    /// (window start, venue_seq, order_id) — the ordering the division rule
    /// names, with the coarsest field first.
    fn eligible_set(&self, block: &BlockFill) -> Vec<Claimant> {
        let mut eligible: Vec<Claimant> = Vec::new();
        for order_id in &block.covers {
            let Some(index) = self.claimant_index(order_id, block.at) else {
                continue;
            };
            let record = &self.submissions[index];
            let outstanding = record.outstanding();
            if outstanding <= 0 {
                continue;
            }
            eligible.push(Claimant {
                index,
                outstanding,
                window_start: record.window_start,
                venue_seq: record.venue_seq,
                order_id: order_id.clone(),
            });
        }
        eligible.sort_by(|left, right| {
            left.window_start
                .cmp(&right.window_start)
                .then(left.venue_seq.cmp(&right.venue_seq))
                .then(left.order_id.cmp(&right.order_id))
        });
        // A label repeated in `covers` is still one order: it takes one share and
        // at most one remainder unit.
        eligible.dedup_by_key(|claimant| claimant.index);
        eligible
    }

    /// The submission a block may allocate to under one of its covered labels:
    /// one still open at the block's match timestamp. A record the venue has
    /// already closed is not eligible, and neither is one whose window the
    /// block's timestamp falls outside. Eligibility decides what an order is
    /// allocated; it does not decide whether the venue closed it.
    fn claimant_index(&self, order_id: &OrderId, at: Timestamp) -> Option<usize> {
        self.indices_of_order(order_id)
            .into_iter()
            .filter(|index| {
                let record = &self.submissions[*index];
                !record.finalized && Self::in_window(record, at)
            })
            .max_by_key(|index| (self.submissions[*index].window_start, *index))
    }

    /// A block is final for every order it names, whatever it gave them.
    ///
    /// The venue has closed each of them and cancelled whatever the block did
    /// not fill, so none of them will ever acquire another base unit, none is
    /// live, and none is still working.
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
    /// about what is live right now, an identifier that is live is by definition
    /// still the order the engine tracked, and so no window is tested against
    /// them. `filled_qty` is the same running total a fill event reports, seen
    /// from the other side — one figure about one order, never a second fill —
    /// so it raises the high-water mark and never adds to it. The entry also
    /// supplies a `venue_seq` for an order the engine had not been told one for.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) =
                self.resolve(&open_order.client_ref, &open_order.order_id, None)
            else {
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
