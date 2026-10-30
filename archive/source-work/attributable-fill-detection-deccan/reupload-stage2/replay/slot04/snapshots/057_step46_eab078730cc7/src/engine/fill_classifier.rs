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
    /// leaves the existing attribution state alone.
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

    /// Whether this engine is holding a record for this reference at all.
    pub fn has(&self, client_ref: &ClientRef) -> bool {
        self.record(client_ref).is_some()
    }

    /// Whether the venue has closed this order, so it can acquire nothing more.
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

    /// Resolve an order id the venue named to one of our submissions, using the
    /// match time to choose between them.
    ///
    /// The venue recycles identifiers once a window has closed, so one id can
    /// name two of this engine's own orders at different times. The window is
    /// what makes an id usable at all, so the record whose window contains the
    /// match is the one this observation is talking about.
    fn index_of_order_at(&self, order_id: &OrderId, at: Timestamp) -> Option<usize> {
        self.submissions
            .iter()
            .position(|record| {
                record.order_id.as_ref() == Some(order_id) && Self::in_window(record, at)
            })
            .or_else(|| self.index_of_order(order_id))
    }

    /// Resolve an observation to one of our submissions.
    ///
    /// The engine's own `client_ref` is the primary key: it is the one key the
    /// engine always knows, it is the venue's idempotency key, and the venue
    /// never recycles it. The venue's `order_id` is only a fallback, because the
    /// venue does recycle it once a window has closed — an observation naming a
    /// recycled id carries some other order's `client_ref` and is not evidence
    /// about this one.
    fn locate(&mut self, order_id: &OrderId, client_ref: &ClientRef) -> Option<usize> {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            return Some(index);
        }
        self.index_of_order(order_id)
    }

    /// Whether a venue match timestamp falls in this record's attribution window.
    ///
    /// The window is half-open: `[start, start + ATTRIBUTION_WINDOW)`. An
    /// observation exactly at the start belongs to the order; one exactly at the
    /// end names an identifier the venue is already free to have reused.
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
        let index = match self.index_by_client_ref.get(&event.client_ref).copied() {
            Some(index) => index,
            // An observation whose client reference this engine does not know.
            // The venue recycles identifiers, so an id alone names an order only
            // through the window the match falls in.
            None => match self.index_of_order_at(&event.order_id, event.at) {
                Some(index) => index,
                None => return,
            },
        };
        // The window is tested against the venue's own match timestamp, not the
        // cycle that happened to read the event: the feed lags and replays.
        if !Self::in_window(&self.submissions[index], event.at) {
            return;
        }
        let record = &mut self.submissions[index];
        if record.order_id.is_none() {
            record.order_id = Some(event.order_id.clone());
        }
        // `cumulative_filled_qty` is the order's running total, not the size of
        // this match, so it raises a high-water mark rather than adding to one.
        // The feed delivers at least once, and both a replay and an out-of-order
        // arrival therefore have to leave the figure where it is.
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
        // idempotency key, so this is what stops a block being counted twice.
        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }

        // Every tracked order the block names, resolved through the match time:
        // the venue recycles identifiers, so the window is what decides which of
        // this engine's orders an id is naming here.
        let mut covered: Vec<usize> = Vec::new();
        for order_id in &block.covers {
            let Some(index) = self.index_of_order_at(order_id, block.at) else {
                continue;
            };
            if !covered.contains(&index) {
                covered.push(index);
            }
        }

        // Eligible set: window containing the block's match time, and still
        // short. An order the venue has already closed has nothing left
        // outstanding, so it contributes no basis and receives no share.
        let mut eligible: Vec<usize> = Vec::new();
        for index in &covered {
            let record = &self.submissions[*index];
            if record.finalized {
                continue;
            }
            if !Self::in_window(record, block.at) {
                continue;
            }
            if record.outstanding() <= 0 {
                continue;
            }
            eligible.push(*index);
        }

        // `covers` arrives in whatever order the venue happened to build it and
        // the outcome must not depend on that, so the eligible set is put in the
        // order the specification states before anything is divided: window
        // start, then venue sequence, then order id.
        eligible.sort_by(|left, right| {
            Self::division_order(&self.submissions[*left])
                .cmp(&Self::division_order(&self.submissions[*right]))
        });

        let outstanding: Vec<Qty> = eligible
            .iter()
            .map(|index| self.submissions[*index].outstanding())
            .collect();

        // Quantity above the basis belongs to orders this engine does not track.
        let basis: Qty = outstanding.iter().sum();
        if basis <= 0 {
            self.finalize(&covered);
            return;
        }
        let divisible = if block.block_qty < basis {
            block.block_qty
        } else {
            basis
        };

        let mut shares: Vec<Qty> = outstanding
            .iter()
            .map(|outstanding| proportional_share(divisible, *outstanding, basis))
            .collect();

        // Truncation leaves a few base units unassigned. The first `r` orders in
        // the ordering above take one each — not the largest order, and not the
        // order with the largest truncated fraction.
        let assigned: Qty = shares.iter().sum();
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

        self.finalize(&covered);
    }

    /// Where an order stands in the block division ordering: window start, then
    /// venue sequence, then order id, each ascending.
    fn division_order(record: &SubmissionRecord) -> (Timestamp, u64, OrderId) {
        (
            record.window_start,
            record.venue_seq.unwrap_or(0),
            record.order_id.clone().unwrap_or_else(|| OrderId::new("")),
        )
    }

    /// A block is final for every order it names, whatever it gave them.
    ///
    /// Eligibility decides what an order is allocated; it does not decide whether
    /// the venue closed it. An order the division skipped — outside its window,
    /// or with nothing outstanding — is closed by the venue just the same.
    fn finalize(&mut self, covered: &[usize]) {
        for index in covered {
            self.submissions[*index].finalized = true;
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp: `open_orders` is point-in-time truth
    /// about what is live right now, and it also supplies a `venue_seq` for an
    /// order the engine had not been told one for.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) = self.locate(&open_order.order_id, &open_order.client_ref) else {
                continue;
            };
            let record = &mut self.submissions[index];
            if record.order_id.is_none() {
                record.order_id = Some(open_order.order_id.clone());
            }
            if record.venue_seq.is_none() {
                record.venue_seq = Some(open_order.venue_seq);
            }
            // `filled_qty` is the same running total a fill event reports as
            // `cumulative_filled_qty`, seen from the other side. One figure about
            // one order, so it raises the high-water mark; folding it in on top
            // of the feed's figure would count the same base units twice. No
            // window is tested: an entry carries no match timestamp, and an
            // identifier that is live is by definition still this order.
            if open_order.filled_qty > record.direct_filled {
                record.direct_filled = open_order.filled_qty;
            }
        }
    }
}
