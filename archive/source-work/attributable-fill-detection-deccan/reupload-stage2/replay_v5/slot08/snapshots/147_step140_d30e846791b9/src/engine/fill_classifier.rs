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
//!
//! A record is identified by the `client_ref` the engine chose. That is the only
//! key the engine always knows: the venue's `order_id` is unknown until the venue
//! names it, and an order whose acknowledgement was lost has none. Once the venue
//! does name an `order_id` for a `client_ref`, either key resolves to the same
//! record — but only while that record's attribution window is open, because the
//! venue recycles identifiers once a window has closed.

use std::collections::{BTreeMap, BTreeSet};

use crate::ports::{BlockFill, FillBatch, FillEvent, OpenOrder};
use crate::types::ids::StrategyId;
use crate::types::time::Timestamp;
use crate::types::ids::{proportional_share, BlockRef, ClientRef, OrderId, Qty, Venue};
use crate::types::pending_order::ATTRIBUTION_WINDOW;
use crate::types::snapshot::{Cursor, OrderRole, SubmissionRecord};
use crate::types::ids::StrategyId;
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

    /// Begin tracking an order this engine has just submitted.
    ///
    /// `window_start` is the instant the engine began tracking it, and a retry
    /// under the same `client_ref` does not reopen the window: the attribution
    /// window of a retried order is the one that opened at the first attempt.
    pub fn push(&mut self, record: SubmissionRecord) {
        self.index_by_client_ref
            .insert(record.client_ref.clone(), self.submissions.len());
        self.submissions.push(record);
    }

    /// Record the identifier and tick the venue has now supplied for an order.
    ///
    /// Used when a resubmission under the same `client_ref` is answered, and it
    /// fills in nothing the engine already knew.
    pub fn note_identity(
        &mut self,
        client_ref: &ClientRef,
        order_id: Option<OrderId>,
        venue_seq: Option<u64>,
    ) {
        let Some(index) = self.index_by_client_ref.get(client_ref).copied() else {
            return;
        };
        let record = &mut self.submissions[index];
        if record.order_id.is_none() {
            record.order_id = order_id;
        }
        if record.venue_seq.is_none() {
            record.venue_seq = venue_seq;
        }
    }

    pub fn set_closing(&mut self, client_ref: &ClientRef, closing: bool) {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            self.submissions[index].closing = closing;
        }
    }

    pub fn set_cancelled(&mut self, client_ref: &ClientRef, cancelled: bool) {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            self.submissions[index].cancelled = cancelled;
        }
    }

    pub fn set_retired(&mut self, client_ref: &ClientRef, retired: bool) {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            self.submissions[index].retired = retired;
        }
    }

    /// Stop tracking an order. Used when the attempt that minted the reference
    /// created nothing at the venue, so a later attempt is a different order.
    pub fn forget(&mut self, client_ref: &ClientRef) {
        let Some(index) = self.index_by_client_ref.get(client_ref).copied() else {
            return;
        };
        self.submissions.remove(index);
        self.reindex();
    }

    /// Stop tracking everything belonging to a mandate that has settled.
    ///
    /// Once a mandate has settled, nothing keyed by its identifiers may
    /// influence a later order that comes to wear one of them.
    pub fn forget_strategy(&mut self, strategy_id: &StrategyId) {
        self.submissions
            .retain(|record| &record.strategy_id != strategy_id);
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
    pub fn outstanding_for(&self, client_ref: &ClientRef) -> Qty {
        match self.record(client_ref) {
            Some(record) if record.finalized => 0,
            Some(record) => record.outstanding(),
            None => 0,
        }
    }

    /// The venue's id for a submission, once anything has reported one.
    pub fn order_for(&self, client_ref: &ClientRef) -> Option<OrderId> {
        self.record(client_ref)
            .and_then(|record| record.order_id.clone())
    }

    pub fn record(&self, client_ref: &ClientRef) -> Option<&SubmissionRecord> {
        self.index_by_client_ref
            .get(client_ref)
            .and_then(|index| self.submissions.get(*index))
    }

    /// Whether the venue has closed this submission, so that it is neither live
    /// nor still working.
    pub fn is_finalized(&self, client_ref: &ClientRef) -> bool {
        self.record(client_ref).map(|record| record.finalized).unwrap_or(false)
    }

    /// Resolve an observation to one of our submissions by the client reference
    /// the engine chose.
    ///
    /// The client reference is the engine's own idempotency key and is never
    /// recycled, so it is authoritative. Resolving this way is also what lets an
    /// observation reach a submission the venue never acknowledged, and what
    /// keeps an observation about an order that now wears a recycled identifier
    /// away from the settled order that used to wear it.
    fn locate(&mut self, order_id: &OrderId, client_ref: &ClientRef) -> Option<usize> {
        let index = self.index_by_client_ref.get(client_ref).copied()?;
        if self.submissions[index].order_id.is_none() {
            self.submissions[index].order_id = Some(order_id.clone());
        }
        Some(index)
    }

    /// Resolve a venue identifier to one of our submissions, for an observation
    /// that carries no client reference — a block match, which names only
    /// identifiers.
    ///
    /// The venue recycles identifiers once a window has closed, so an identifier
    /// alone is ambiguous across time: it is usable only inside the window of the
    /// order that currently wears it. This is the resolution that decides what a
    /// block *allocates*, so it is the strict one.
    fn resolve_by_order_at(&self, order_id: &OrderId, at: Timestamp) -> Option<usize> {
        self.latest_wearing(order_id, |record| {
            !record.retired && Self::in_window(record, at)
        })
    }

    /// Resolve a venue identifier for the purpose of *finality* rather than
    /// allocation.
    ///
    /// A block closes every tracked order it names, including one the division
    /// skipped because the match timestamp fell outside that order's attribution
    /// window. Finality is a statement about the venue rather than about the
    /// arithmetic, so the window may not gate it. The in-window record is still
    /// preferred, because that is the order the identifier means right now;
    /// otherwise the most recently opened record wearing the identifier is the
    /// only tracked order it can refer to.
    fn resolve_for_finality(&self, order_id: &OrderId, at: Timestamp) -> Option<usize> {
        self.resolve_by_order_at(order_id, at)
            .or_else(|| self.latest_wearing(order_id, |record| !record.retired))
    }

    /// The most recently opened record wearing `order_id` that satisfies
    /// `accept`. Ties go to the later position, so the answer never depends on
    /// the order the container happens to hold records in.
    fn latest_wearing(
        &self,
        order_id: &OrderId,
        accept: impl Fn(&SubmissionRecord) -> bool,
    ) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (index, record) in self.submissions.iter().enumerate() {
            if record.order_id.as_ref() != Some(order_id) || !accept(record) {
                continue;
            }
            best = match best {
                Some(current) if self.submissions[current].window_start > record.window_start => {
                    Some(current)
                }
                _ => Some(index),
            };
        }
        best
    }

    /// Whether a venue match timestamp falls in this record's attribution window.
    ///
    /// The window is half-open: an observation stamped exactly at
    /// `window_start + ATTRIBUTION_WINDOW` names an identifier the venue is
    /// already free to have handed to an unrelated order.
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
        let Some(index) = self.locate(&event.order_id, &event.client_ref) else {
            return;
        };
        if !Self::in_window(&self.submissions[index], event.at) {
            return;
        }
        // `cumulative_filled_qty` is that order's running total, not the size of
        // this match, so redelivery must not add it again: direct progress is the
        // maximum ever observed.
        let record = &mut self.submissions[index];
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
        // The same match, redelivered, is the same match.
        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }

        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut eligible: Vec<usize> = Vec::new();
        for order_id in &block.covers {
            let Some(index) = self.resolve_by_order_at(order_id, block.at) else {
                continue;
            };
            if !seen.insert(index) {
                continue;
            }
            // An order the venue has already closed contributes no outstanding
            // quantity to a later block's basis.
            if !self.submissions[index].finalized && self.submissions[index].outstanding() > 0 {
                eligible.push(index);
            }
        }

        // A block is final for every order it names, so finalize whether or not
        // the division had anything to allocate: the venue closed them all.
        if eligible.is_empty() {
            self.finalize(&block.covers, block.at);
            return;
        }

        let basis: Qty = eligible
            .iter()
            .map(|index| self.submissions[*index].outstanding())
            .sum();
        if basis <= 0 {
            self.finalize(&block.covers, block.at);
            return;
        }

        // Quantity above the basis belongs to orders this engine does not track.
        let divisible = if block.block_qty < basis {
            block.block_qty
        } else {
            basis
        };
        if divisible <= 0 {
            self.finalize(&block.covers, block.at);
            return;
        }

        let mut shares: Vec<Qty> = eligible
            .iter()
            .map(|index| proportional_share(divisible, self.submissions[*index].outstanding(), basis))
            .collect();
        let assigned: Qty = shares.iter().sum();

        // Truncation leaves a few base units unassigned. They go one each to the
        // first orders in the documented ordering, never to the order with the
        // largest leftover fraction.
        let mut remainder = divisible - assigned;
        if remainder > 0 {
            let mut ordered = eligible.clone();
            ordered.sort_by(|left, right| self.ordering_key(*left).cmp(&self.ordering_key(*right)));
            for index in ordered {
                if remainder <= 0 {
                    break;
                }
                if let Some(position) = eligible.iter().position(|candidate| *candidate == index) {
                    shares[position] += 1;
                    remainder -= 1;
                }
            }
        }

        for (position, index) in eligible.iter().enumerate() {
            self.submissions[*index].block_filled += shares[position];
        }

        self.finalize(&block.covers, block.at);
    }

    /// The ordering the division rule prescribes: window start, then the venue
    /// tick that accepted the order, then the venue identifier.
    fn ordering_key(&self, index: usize) -> (Timestamp, Option<u64>, OrderId) {
        let record = &self.submissions[index];
        (
            record.window_start,
            record.venue_seq,
            record.order_id.clone().unwrap_or_else(|| OrderId::new("")),
        )
    }

    /// A block is final for every tracked order it names, whatever it gave them.
    ///
    /// Finalization is a statement about the venue rather than about the
    /// arithmetic: the venue has closed the order and cancelled whatever part of
    /// it the block did not fill. Eligibility decides what an order is
    /// *allocated*; it does not decide whether the venue closed it.
    fn finalize(&mut self, covers: &[OrderId], at: Timestamp) {
        let mut indices: Vec<usize> = Vec::new();
        for order_id in covers {
            if let Some(index) = self.resolve_for_finality(order_id, at) {
                if !indices.contains(&index) {
                    indices.push(index);
                }
            }
        }
        for index in indices {
            self.submissions[index].finalized = true;
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp: `open_orders` is point-in-time truth
    /// about what is live right now, and an identifier that is live is by
    /// definition still the order the engine tracked, so the window does not
    /// apply. The list also supplies a `venue_seq` for an order the engine had
    /// not been told one for.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) = self.locate(&open_order.order_id, &open_order.client_ref) else {
                continue;
            };
            let record = &mut self.submissions[index];
            if record.venue_seq.is_none() {
                record.venue_seq = Some(open_order.venue_seq);
            }
            // `filled_qty` is the same figure a fill event reports as
            // `cumulative_filled_qty`, seen from the other side: one quantity
            // about one order, never an increment on top of the feed.
            if open_order.filled_qty > record.direct_filled {
                record.direct_filled = open_order.filled_qty;
            }
        }
    }

    /// Whether this mandate already has an order resting on `venue`.
    ///
    /// True whether or not the venue has named it: an order whose
    /// acknowledgement was lost is tracked and must not be placed again.
    pub fn has_leg(&self, strategy_id: &StrategyId, venue: Venue, role: OrderRole) -> bool {
        self.submissions.iter().any(|record| {
            record.role == role
                && !record.retired
                && &record.strategy_id == strategy_id
                && record.venue == venue
        })
    }

    /// The tracked order for one mandate and venue, once the venue has named it.
    pub fn leg(&self, strategy_id: &StrategyId, venue: Venue) -> Option<&SubmissionRecord> {
        self.submissions.iter().find(|record| {
            record.role == OrderRole::Entry
                && !record.retired
                && &record.strategy_id == strategy_id
                && record.venue == venue
                && record.is_named()
        })
    }

    /// Every neutralization this engine has committed to for one mandate and
    /// venue, whether or not the venue has named it.
    pub fn neutralizations_for(&self, strategy_id: &StrategyId, venue: Venue) -> Vec<&SubmissionRecord> {
        self.submissions
            .iter()
            .filter(|record| {
                record.role == OrderRole::Neutralization
                    && &record.strategy_id == strategy_id
                    && record.venue == venue
            })
            .collect()
    }

    /// Mandates both of whose legs are tracked and named, so that completion can
    /// be judged. Sorted: the order in which mandates are reconciled must not
    /// depend on the container the records happen to sit in.
    pub fn judgeable_strategy_ids(&self) -> Vec<StrategyId> {
        let mut ids: Vec<StrategyId> = self
            .submissions
            .iter()
            .filter(|record| record.role == OrderRole::Entry && !record.retired && record.is_named())
            .map(|record| record.strategy_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids.retain(|id| {
            self.leg(id, Venue::North).is_some() && self.leg(id, Venue::South).is_some()
        });
        ids
    }

    /// Orders this engine has submitted whose identifier the venue has never
    /// given it, so that a retry under the same client reference is still owed.
    pub fn unnamed(&self) -> Vec<SubmissionRecord> {
        self.submissions
            .iter()
            .filter(|record| !record.is_named() && !record.retired)
            .cloned()
            .collect()
    }
}
