//! Per-order fill attribution.
//!
//! Custody inventory is shared, absolute and lagging, so it can never prove that
//! one specific order filled. This ledger instead builds up, per order the engine
//! itself submitted, the quantity that is genuinely attributable to it:
//!
//! - in-window fill events, which carry a *cumulative* quantity, so the ledger
//!   keeps a maximum rather than a sum;
//! - in-window shares of block fills, which are incremental and deduplicated by
//!   `block_ref`, so the ledger adds them;
//! - `filled_qty` from the open-order list, which is point-in-time truth and
//!   carries no timestamp.
//!
//! A record is keyed by the client reference the engine chose: the venue's
//! identifier is learned later, may be unknown for an order whose acknowledgement
//! was lost, and is recycled by the venue once a window has closed. Every
//! timestamped observation is tested against the order's half-open attribution
//! window before it counts, and a venue identifier alone resolves to the tracked
//! order whose window contains the observation.

use std::collections::{BTreeMap, BTreeSet};

use crate::ports::{BlockFill, FillBatch, FillEvent, OpenOrder};
use crate::types::ids::{proportional_share, BlockRef, ClientRef, OrderId, Qty};
use crate::types::pending_order::ATTRIBUTION_WINDOW;
use crate::types::snapshot::{Cursor, SubmissionRecord};
use crate::types::time::Timestamp;

/// Sort key for the block division tie-break, coarsest field first.
///
/// Window start orders arrivals across venue ticks; `venue_seq` orders them
/// within one timestamp; `order_id` settles orders accepted in the same tick.
/// An unknown `venue_seq` sorts last, so an order whose acknowledgement was lost
/// never displaces a confirmed one.
type TieBreak = (Timestamp, u64, OrderId);

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

    /// Record an order the engine has committed to. The window opens now.
    ///
    /// Called again for the same `client_ref` — a retry under the venue's
    /// idempotency key — this fills in whatever the venue has since supplied and
    /// leaves the original window start and the attribution accumulated so far
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

    pub fn order_for(&self, client_ref: &ClientRef) -> Option<OrderId> {
        self.record(client_ref).and_then(|record| record.order_id.clone())
    }

    pub fn venue_seq_for(&self, client_ref: &ClientRef) -> Option<u64> {
        self.record(client_ref).and_then(|record| record.venue_seq)
    }

    /// Whether the venue has closed this submission by naming it in a block.
    pub fn is_finalized(&self, client_ref: &ClientRef) -> bool {
        self.record(client_ref)
            .map(|record| record.finalized)
            .unwrap_or(false)
    }

    fn record(&self, client_ref: &ClientRef) -> Option<&SubmissionRecord> {
        self.index_by_client_ref
            .get(client_ref)
            .and_then(|index| self.submissions.get(*index))
    }

    fn in_window(record: &SubmissionRecord, at: Timestamp) -> bool {
        at >= record.window_start && at < record.window_start + ATTRIBUTION_WINDOW
    }

    /// The tracked order a venue identifier names at instant `at`.
    ///
    /// The venue recycles identifiers once a window has closed, so an identifier
    /// alone is ambiguous across time; inside a window it is not. Prefer the
    /// record whose window contains the instant, otherwise the newest holder.
    fn index_of_order_at(&self, order_id: &OrderId, at: Option<Timestamp>) -> Option<usize> {
        let mut newest = None;
        for (index, record) in self.submissions.iter().enumerate() {
            if record.order_id.as_ref() != Some(order_id) {
                continue;
            }
            if let Some(at) = at {
                if Self::in_window(record, at) {
                    return Some(index);
                }
            }
            newest = Some(index);
        }
        newest
    }

    /// Resolve an observation to one of the engine's own submissions.
    ///
    /// The client reference is the primary key: it is the only one the engine
    /// always knows, and it is what recovers an order whose acknowledgement was
    /// lost. The venue identifier is a fallback, resolved at the observation's
    /// own instant.
    fn locate(&self, client_ref: &ClientRef, order_id: &OrderId, at: Option<Timestamp>) -> Option<usize> {
        let _ = client_ref;
        self.index_of_order_at(order_id, at)
    }

    /// Learn the venue's identifier for a submission from an observation that is
    /// genuinely about it. Only an unknown identifier is filled in.
    fn bind_order_id(&mut self, index: usize, order_id: &OrderId) {
        if self.submissions[index].order_id.is_none() {
            self.submissions[index].order_id = Some(order_id.clone());
        }
    }

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
        let Some(index) = self.locate(&event.client_ref, &event.order_id, Some(event.at)) else {
            return;
        };
        if !Self::in_window(&self.submissions[index], event.at) {
            return;
        }
        self.bind_order_id(index, &event.order_id);
        let record = &mut self.submissions[index];
        // A block has closed this order; it acquires nothing more.
        if record.finalized {
            return;
        }
        // Cumulative, not incremental: keep the high-water mark so a replayed or
        // out-of-order event can never add quantity twice.
        if event.cumulative_filled_qty > record.direct_filled {
            record.direct_filled = event.cumulative_filled_qty;
        }
    }

    /// Divide one block match across the orders it covers.
    ///
    /// The rule is in `/app/spec/venue-semantics.md`. Every step is exact integer
    /// arithmetic: no base unit is invented, and any part of the block that no
    /// tracked order can account for is dropped rather than assigned.
    fn absorb_block(&mut self, block: BlockFill) {
        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }

        // Tracked orders the block names, each once however `covers` lists it,
        // resolved at the block's own match instant.
        let mut covered: Vec<usize> = Vec::new();
        for order_id in &block.covers {
            if let Some(index) = self.index_of_order_at(order_id, Some(block.at)) {
                if !covered.contains(&index) {
                    covered.push(index);
                }
            }
        }

        let mut eligible: Vec<(TieBreak, usize, Qty)> = Vec::new();
        for &index in &covered {
            let record = &self.submissions[index];
            if record.finalized || !Self::in_window(record, block.at) {
                continue;
            }
            let outstanding = record.outstanding();
            if outstanding <= 0 {
                continue;
            }
            let key = (
                record.window_start,
                record.venue_seq.unwrap_or(u64::MAX),
                record.order_id.clone().unwrap_or_else(|| OrderId::new(String::new())),
            );
            eligible.push((key, index, outstanding));
        }

        // The documented ordering decides the outcome. Nothing else may: not the
        // order of `covers`, and not the iteration order of any container.
        eligible.sort_by(|left, right| left.0.cmp(&right.0));

        let basis: Qty = eligible.iter().map(|(_, _, outstanding)| *outstanding).sum();
        if basis <= 0 {
            self.finalize(&covered);
            return;
        }

        let divisible = if block.block_qty < basis {
            block.block_qty
        } else {
            basis
        };

        let mut assigned: Qty = 0;
        let mut shares: Vec<Qty> = Vec::with_capacity(eligible.len());
        for (_, _, outstanding) in &eligible {
            let share = proportional_share(divisible, *outstanding, basis);
            assigned += share;
            shares.push(share);
        }

        // Truncation always leaves fewer base units than there are eligible
        // orders, so one extra unit each, earliest first, exactly exhausts it.
        let mut remainder = divisible - assigned;
        for share in shares.iter_mut() {
            if remainder <= 0 {
                break;
            }
            *share += 1;
            remainder -= 1;
        }

        for ((_, index, _), share) in eligible.iter().zip(shares) {
            self.submissions[*index].block_filled += share;
        }

        self.finalize(&covered);
    }

    /// A block closes every order it names, whatever it gave them.
    fn finalize(&mut self, covered: &[usize]) {
        for &index in covered {
            self.submissions[index].finalized = true;
        }
    }

    /// The open-order list is point-in-time truth and carries no timestamp, so
    /// the attribution window does not apply to it.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) = self.locate(&open_order.client_ref, &open_order.order_id, None) else {
                continue;
            };
            self.bind_order_id(index, &open_order.order_id);
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
