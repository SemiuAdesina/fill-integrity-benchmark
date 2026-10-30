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
use crate::types::ids::{proportional_share, BlockRef, ClientRef, OrderId, Qty, Venue};
use crate::types::snapshot::{AttemptState, SubmissionRecord, SubmissionRole};
use crate::types::time::Timestamp;

#[derive(Debug, Clone, Default)]
pub struct AttributionLedger {
    submissions: Vec<SubmissionRecord>,
    index_by_client_ref: BTreeMap<ClientRef, usize>,
    absorbed_blocks: BTreeSet<BlockRef>,
    cursor: Cursor,
}

use crate::types::snapshot::Cursor;

impl AttributionLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuild a ledger from the fields a `Snapshot` carried across a restart.
    ///
    /// The window start each record carries is restored unchanged: a restart
    /// does not reopen a window, and an observation from outside the original
    /// one must stay outside it.
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

    /// Start tracking an order under the client reference the engine chose.
    ///
    /// The record is created before the venue is asked anything, so an order
    /// whose acknowledgement is lost is still something an observation naming
    /// that reference can reach.
    pub fn track(&mut self, record: SubmissionRecord) {
        if self.index_by_client_ref.contains_key(&record.client_ref) {
            return;
        }
        self.index_by_client_ref
            .insert(record.client_ref.clone(), self.submissions.len());
        self.submissions.push(record);
    }

    pub fn remove(&mut self, client_ref: &ClientRef) {
        if let Some(index) = self.index_by_client_ref.remove(client_ref) {
            self.submissions.remove(index);
            self.reindex();
        }
    }

    /// Drop every record for a mandate that has settled. Nothing keyed by its
    /// identifiers may influence a later order that comes to wear one of them.
    pub fn forget_strategy(&mut self, strategy_id: &crate::types::ids::StrategyId) {
        self.submissions
            .retain(|record| &record.strategy_id != strategy_id);
        self.reindex();
    }

    pub fn mark_acknowledged(
        &mut self,
        client_ref: &ClientRef,
        order_id: OrderId,
        venue_seq: u64,
        window_start: Timestamp,
    ) {
        if let Some(record) = self.record_mut(client_ref) {
            record.order_id = Some(order_id);
            record.venue_seq = Some(venue_seq);
            record.attempt = AttemptState::Acknowledged;
            record.window_start = window_start;
        }
    }

    /// The venue refused the submission outright, so nothing was created.
    pub fn mark_failed(&mut self, client_ref: &ClientRef) {
        if let Some(record) = self.record_mut(client_ref) {
            record.attempt = AttemptState::Failed;
            record.order_id = None;
        }
    }

    pub fn mark_cancelled(&mut self, client_ref: &ClientRef) {
        if let Some(record) = self.record_mut(client_ref) {
            record.cancelled = true;
        }
    }

    pub fn mark_retired(&mut self, client_ref: &ClientRef) {
        if let Some(record) = self.record_mut(client_ref) {
            record.retired = true;
        }
    }

    /// Quantity attributed to one submission so far.
    pub fn attributed_for(&self, client_ref: &ClientRef) -> Qty {
        self.record(client_ref)
            .map(SubmissionRecord::attributed)
            .unwrap_or(0)
    }

    /// What a committed submission still covers, and so must not be corrected a
    /// second time.
    ///
    /// - one the venue closed covers nothing, whatever it managed to fill, and
    ///   the gap it leaves behind is real;
    /// - one whose acknowledgement was lost covers its full size, because the
    ///   venue may have created it and the engine cannot yet see it;
    /// - one the venue refused covers nothing at all.
    pub fn working_for(&self, client_ref: &ClientRef) -> Qty {
        match self.record(client_ref) {
            None => 0,
            Some(record) if record.is_closed() => 0,
            Some(record) => match record.attempt {
                AttemptState::Failed => 0,
                AttemptState::Unconfirmed => record.size,
                AttemptState::Acknowledged => record.outstanding(),
            },
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

    fn record_mut(&mut self, client_ref: &ClientRef) -> Option<&mut SubmissionRecord> {
        let index = self.index_by_client_ref.get(client_ref).copied()?;
        self.submissions.get_mut(index)
    }

    /// Resolve an observation to one of our submissions.
    ///
    /// The client reference comes first: it is the key the engine chose, the
    /// venue never recycles it, and it is the only key an order whose
    /// acknowledgement was lost has. Falling back to the venue's identifier is
    /// what lets an observation reach a submission the engine never saw named,
    /// and it is window-aware because the venue recycles identifiers once a
    /// window has closed.
    fn resolve(
        &mut self,
        order_id: &OrderId,
        client_ref: &ClientRef,
        at: Option<Timestamp>,
    ) -> Option<usize> {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            let record = &mut self.submissions[index];
            if record.order_id.is_none() {
                record.order_id = Some(order_id.clone());
            }
            return Some(index);
        }
        let mut best: Option<usize> = None;
        for (index, record) in self.submissions.iter().enumerate() {
            if record.retired {
                continue;
            }
            if record.order_id.as_ref() != Some(order_id) {
                continue;
            }
            match at {
                Some(at) if !record.in_window(at) => continue,
                None if record.is_closed() => continue,
                _ => {}
            }
            let better = match best {
                None => true,
                Some(chosen) => record.window_start >= self.submissions[chosen].window_start,
            };
            if better {
                best = Some(index);
            }
        }
        best
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
        let Some(index) = self.resolve(&event.order_id, &event.client_ref, Some(event.at)) else {
            return;
        };
        let record = &mut self.submissions[index];
        // An observation from outside the window names an identifier the venue
        // has recycled, and counting it is a misattribution.
        if !record.in_window(event.at) {
            return;
        }
        // The venue closed this order; it acquires nothing more.
        if record.is_closed() {
            return;
        }
        // Direct progress is one cumulative figure seen from two sides, so it
        // folds in as a maximum. Adding either side on top of the other counts
        // the same base units twice.
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

        // Eligible set: tracked, named, inside its window at the block's own
        // match instant, with something still outstanding.
        let mut eligible: Vec<usize> = Vec::new();
        for order_id in &block.covers {
            for (index, record) in self.submissions.iter().enumerate() {
                if record.retired || record.is_closed() {
                    continue;
                }
                if record.order_id.as_ref() != Some(order_id) {
                    continue;
                }
                if !record.in_window(block.at) {
                    continue;
                }
                if record.outstanding() <= 0 {
                    continue;
                }
                if !eligible.contains(&index) {
                    eligible.push(index);
                }
            }
        }

        // The ordering is part of the rule: window start, then the venue tick
        // that accepted the order, then the identifier. Nothing here looks at
        // the order the venue happened to list the covers in.
        eligible.sort_by(|left, right| {
            let a = &self.submissions[*left];
            let b = &self.submissions[*right];
            a.window_start
                .cmp(&b.window_start)
                .then(a.venue_seq.cmp(&b.venue_seq))
                .then(a.order_id.cmp(&b.order_id))
        });

        let basis: Qty = eligible
            .iter()
            .map(|index| self.submissions[*index].outstanding())
            .sum();
        if basis > 0 {
            // Quantity above the basis belongs to orders this engine does not
            // track, so it is dropped rather than assigned.
            let divisible = if block.block_qty < basis {
                block.block_qty
            } else {
                basis
            };

            let mut assigned: Qty = 0;
            let mut shares: Vec<Qty> = Vec::with_capacity(eligible.len());
            for index in &eligible {
                let share =
                    proportional_share(divisible, self.submissions[*index].outstanding(), basis);
                assigned += share;
                shares.push(share);
            }

            // Truncation leaves a few base units unassigned, and they go one
            // each to the first orders in the ordering above.
            let remainder = divisible - assigned;
            let mut extra = remainder;
            for share in shares.iter_mut() {
                if extra <= 0 {
                    break;
                }
                *share += 1;
                extra -= 1;
            }

            for (position, index) in eligible.iter().enumerate() {
                self.submissions[*index].block_filled += shares[position];
            }
        }

        // A block closes every order it names, whatever it allocated them.
        // Eligibility decides what an order is allocated; it does not decide
        // whether the venue closed it.
        for order_id in &block.covers {
            for record in self.submissions.iter_mut() {
                if record.retired {
                    continue;
                }
                if record.order_id.as_ref() == Some(order_id) {
                    record.finalized = true;
                }
            }
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp: `open_orders` is point-in-time truth
    /// about what is live right now, so the window does not apply to it, and it
    /// also supplies a `venue_seq` for an order the engine had not been told
    /// one for.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) = self.resolve(&open_order.order_id, &open_order.client_ref, None)
            else {
                continue;
            };
            let record = &mut self.submissions[index];
            if record.is_closed() {
                continue;
            }
            if record.venue_seq.is_none() {
                record.venue_seq = Some(open_order.venue_seq);
            }
            // The same cumulative figure a fill event reports, so it folds in
            // as a maximum too.
            if open_order.filled_qty > record.direct_filled {
                record.direct_filled = open_order.filled_qty;
            }
        }
    }

    /// Every submission still being worked, in the order they were tracked.
    pub fn active(&self) -> Vec<SubmissionRecord> {
        self.submissions
            .iter()
            .filter(|record| !record.retired)
            .cloned()
            .collect()
    }

    /// The entry leg one mandate rests on one venue, if it has one still worked.
    pub fn entry(
        &self,
        strategy_id: &crate::types::ids::StrategyId,
        venue: Venue,
    ) -> Option<SubmissionRecord> {
        self.submissions
            .iter()
            .find(|record| {
                !record.retired
                    && record.role == SubmissionRole::Entry
                    && &record.strategy_id == strategy_id
                    && record.venue == venue
            })
            .cloned()
    }

    /// Corrective orders committed for one mandate and venue.
    pub fn corrections(
        &self,
        strategy_id: &crate::types::ids::StrategyId,
        venue: Venue,
    ) -> Vec<SubmissionRecord> {
        self.submissions
            .iter()
            .filter(|record| {
                record.role == SubmissionRole::Neutralization
                    && &record.strategy_id == strategy_id
                    && record.venue == venue
            })
            .cloned()
            .collect()
    }

    /// Mandates that still have entry legs being worked.
    pub fn active_strategy_ids(&self) -> Vec<crate::types::ids::StrategyId> {
        let mut ids: Vec<crate::types::ids::StrategyId> = self
            .submissions
            .iter()
            .filter(|record| !record.retired && record.role == SubmissionRole::Entry)
            .map(|record| record.strategy_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }
}
