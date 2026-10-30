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
//! A record is keyed by the `client_ref` the engine chose, because that is the
//! only key the engine always knows: the venue's `order_id` is unknown until the
//! venue names it, and an order whose acknowledgement was lost has none. The
//! venue recycles identifiers, so `order_id` is only ever a secondary way in,
//! and the most recent record to be given an identifier is the one it resolves
//! to.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::ports::{BlockFill, FillBatch, FillEvent, OpenOrder};
use crate::types::ids::{proportional_share, BlockRef, ClientRef, OrderId, Qty, StrategyId, Venue};
use crate::types::snapshot::{ClosureState, Cursor, OrderRole, SubmissionRecord};
use crate::types::time::Timestamp;

/// One entry of the fill feed, whichever kind it is.
enum Observation {
    Event(FillEvent),
    Block(BlockFill),
}

/// One order a block can allocate to, with everything the ordering rule needs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Eligible {
    window_start: Timestamp,
    venue_seq: u64,
    order_id: OrderId,
    client_ref: ClientRef,
    index: usize,
    outstanding: Qty,
}

impl Eligible {
    /// The order the division rule prescribes: attribution window start, then
    /// the venue tick that accepted the order, then the identifier. The client
    /// reference only breaks a tie that the identifier alone could not, so the
    /// outcome never depends on how the orders happened to be stored.
    fn sort_key(&self) -> (Timestamp, u64, &OrderId, &ClientRef) {
        (self.window_start, self.venue_seq, &self.order_id, &self.client_ref)
    }
}

impl Ord for Eligible {
    fn cmp(&self, other: &Self) -> Ordering {
        self.sort_key().cmp(&other.sort_key())
    }
}

impl PartialOrd for Eligible {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Default)]
pub struct AttributionLedger {
    submissions: Vec<SubmissionRecord>,
    index_by_client_ref: BTreeMap<ClientRef, usize>,
    /// The record an identifier currently resolves to. The venue recycles
    /// identifiers, so the most recent record to be handed one owns it.
    index_by_order_id: BTreeMap<OrderId, usize>,
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
            self.index_by_client_ref.insert(record.client_ref.clone(), index);
            if let Some(order_id) = record.order_id.clone() {
                self.index_by_order_id.insert(order_id, index);
            }
        }
    }

    /// Begin tracking an order the engine has just submitted.
    ///
    /// A `SubmissionRecord` carries the window it opened at the first attempt, so
    /// tracking one whose acknowledgement was lost and tracking it again when the
    /// retry is answered describe the same order.
    pub fn track(&mut self, record: SubmissionRecord) {
        let index = self.submissions.len();
        if let Some(order_id) = record.order_id.clone() {
            self.index_by_order_id.insert(order_id, index);
        }
        self.index_by_client_ref.insert(record.client_ref.clone(), index);
        self.submissions.push(record);
    }

    /// Record what the venue answered to a submission, or to a retry of one.
    pub fn attach(&mut self, client_ref: &ClientRef, order_id: OrderId, venue_seq: u64) {
        let Some(index) = self.index_by_client_ref.get(client_ref).copied() else {
            return;
        };
        let record = &mut self.submissions[index];
        if record.order_id.is_none() {
            record.order_id = Some(order_id.clone());
            self.index_by_order_id.insert(order_id, index);
        }
        if record.venue_seq.is_none() {
            record.venue_seq = Some(venue_seq);
        }
    }

    /// Stop tracking an order. Used when the venue says nothing was created, and
    /// when a mandate settles and no longer wants to be found by its identifier.
    pub fn forget(&mut self, client_ref: &ClientRef) {
        if self.index_by_client_ref.contains_key(client_ref) {
            self.submissions.retain(|record| &record.client_ref != client_ref);
            self.reindex();
        }
    }

    /// Settle a mandate: its orders are done, and nothing keyed by their
    /// identifiers may speak for a later order that comes to wear one of them.
    pub fn settle_strategy(&mut self, strategy_id: &StrategyId) {
        if self
            .submissions
            .iter()
            .any(|record| &record.strategy_id == strategy_id)
        {
            self.submissions
                .retain(|record| &record.strategy_id != strategy_id);
            self.reindex();
        }
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
        self.record(client_ref)
            .map(SubmissionRecord::outstanding)
            .unwrap_or(0)
    }

    /// The venue's id for a submission, once anything has reported one.
    pub fn order_for(&self, client_ref: &ClientRef) -> Option<OrderId> {
        self.record(client_ref).and_then(|record| record.order_id.clone())
    }

    pub fn record(&self, client_ref: &ClientRef) -> Option<&SubmissionRecord> {
        self.index_by_client_ref
            .get(client_ref)
            .and_then(|index| self.submissions.get(*index))
    }

    /// Every order still being tracked, in submission order.
    pub fn records(&self) -> &[SubmissionRecord] {
        &self.submissions
    }

    /// The resting half of a mandate on one venue, if it is still tracked.
    pub fn active_leg(&self, strategy_id: &StrategyId, venue: Venue) -> Option<&SubmissionRecord> {
        self.submissions.iter().find(|record| {
            record.role == OrderRole::Leg
                && &record.strategy_id == strategy_id
                && record.venue == venue
        })
    }

    /// Every tracked resting leg.
    pub fn active_legs(&self) -> impl Iterator<Item = &SubmissionRecord> {
        self.submissions
            .iter()
            .filter(|record| record.role == OrderRole::Leg)
    }

    /// Mandates with a resting leg, in a deterministic order.
    pub fn tracked_strategy_ids(&self) -> Vec<StrategyId> {
        let mut ids: Vec<StrategyId> = self
            .active_legs()
            .map(|record| record.strategy_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// Quantity attributed to one leg of a mandate: the leg's own resting order
    /// and every corrective order sent for the same mandate and venue.
    pub fn acquired(&self, strategy_id: &StrategyId, venue: Venue) -> Qty {
        self.submissions
            .iter()
            .filter(|record| &record.strategy_id == strategy_id && record.venue == venue)
            .map(SubmissionRecord::attributed)
            .sum()
    }

    /// Quantity still working in corrective orders for one leg of a mandate.
    ///
    /// This is what stops consecutive cycles from covering the same exposure
    /// twice. An acknowledged order contributes what it has left to fill; one
    /// whose acknowledgement was lost contributes its whole size, because the
    /// venue may have created it and this engine cannot yet see it; one the
    /// venue has closed contributes nothing, because the gap it leaves behind is
    /// real.
    pub fn working_neutralizations(&self, strategy_id: &StrategyId, venue: Venue) -> Qty {
        self.submissions
            .iter()
            .filter(|record| {
                record.role == OrderRole::Neutralization
                    && &record.strategy_id == strategy_id
                    && record.venue == venue
            })
            .map(|record| {
                if record.finalized {
                    // Closed by the venue: the gap it leaves behind is real.
                    0
                } else if record.order_id.is_none() {
                    // The venue may have created it and this engine cannot see
                    // it, so the whole size counts as working.
                    record.size
                } else {
                    record.outstanding()
                }
            })
            .sum()
    }

    /// Move a resting leg into closure.
    pub fn mark_for_closure(&mut self, client_ref: &ClientRef) {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            self.submissions[index].closure = ClosureState::Marked;
        }
    }

    /// Record that the venue answered a cancellation of this order.
    pub fn mark_cancelled(&mut self, client_ref: &ClientRef) {
        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            self.submissions[index].cancelled = true;
            self.submissions[index].closure = ClosureState::Open;
        }
    }

    /// Resolve an observation to one of our submissions.
    ///
    /// A fill event and an entry of the live order list both echo the
    /// `client_ref` the engine chose, and that reference is the engine's own key:
    /// it is unique, it is never recycled, and an order whose acknowledgement was
    /// lost still has one. So it is the only key needed here, and reaching for
    /// the venue's identifier instead could only ever land the observation on
    /// whichever order last wore a recycled label.
    ///
    /// An observation whose reference is unknown is about an order this engine
    /// does not track, and attributing it to anything would be a misattribution.
    fn locate(&self, client_ref: &ClientRef) -> Option<usize> {
        self.index_by_client_ref.get(client_ref).copied()
    }

    /// The record an identifier in a block's `covers` list names.
    ///
    /// A block reports only identifiers, so this is the one place the venue's
    /// label has to be trusted. Identifiers are recycled, so the most recent
    /// record to be handed one owns it, and a settled mandate's records are gone
    /// from the ledger entirely.
    fn resolve_cover(&self, order_id: &OrderId) -> Option<usize> {
        self.index_by_order_id.get(order_id).copied()
    }

    /// Fold one polled batch into the ledger and advance the cursor.
    ///
    /// Returns the number of per-order events in the batch, which the engine
    /// reports through `Engine::observed_fill_events`.
    pub fn absorb_batch(&mut self, batch: FillBatch) -> usize {
        let count = batch.events.len();
        // The feed is one ordered stream: `seq` is strictly increasing, so a
        // batch is folded in the order the venue reported it rather than by
        // kind.
        let mut observations: Vec<(u64, Observation)> = batch
            .events
            .into_iter()
            .map(|event| (event.seq, Observation::Event(event)))
            .chain(
                batch
                    .blocks
                    .into_iter()
                    .map(|block| (block.seq, Observation::Block(block))),
            )
            .collect();
        observations.sort_by_key(|(seq, _)| *seq);
        for (_, observation) in observations {
            match observation {
                Observation::Event(event) => self.absorb_event(event),
                Observation::Block(block) => self.absorb_block(block),
            }
        }
        self.cursor = batch.next_cursor;
        count
    }

    fn absorb_event(&mut self, event: FillEvent) {
        let Some(index) = self.locate(&event.client_ref) else {
            return;
        };
        // The window is tested against the venue's own match timestamp, not
        // against the cycle in which the feed happened to deliver the event.
        if !self.submissions[index].in_window(event.at) {
            return;
        }
        let record = &mut self.submissions[index];
        // Direct progress is cumulative: the event states the order's running
        // total, so it replaces what is held rather than adding to it.
        if event.cumulative_filled_qty > record.direct_filled {
            record.direct_filled = event.cumulative_filled_qty;
        }
        if record.order_id.is_none() {
            record.order_id = Some(event.order_id.clone());
            self.index_by_order_id.insert(event.order_id, index);
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

        // Eligible: tracked, inside its own window at the block's match instant,
        // and still short of its size.
        let mut eligible: Vec<Eligible> = Vec::new();
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        for order_id in &block.covers {
            let Some(index) = self.resolve_cover(order_id) else {
                continue;
            };
            // One order, one share, however many times the venue lists it.
            if !seen.insert(index) {
                continue;
            }
            let record = &self.submissions[index];
            if !record.in_window(block.at) {
                continue;
            }
            let outstanding = record.outstanding();
            if outstanding <= 0 {
                continue;
            }
            eligible.push(Eligible {
                window_start: record.window_start,
                venue_seq: record.venue_seq.unwrap_or(0),
                order_id: record.order_id.clone().unwrap_or_else(|| order_id.clone()),
                client_ref: record.client_ref.clone(),
                index,
                outstanding,
            });
        }

        let basis: Qty = eligible.iter().map(|entry| entry.outstanding).sum();
        if basis <= 0 {
            // Nothing here can account for the block, so it attributes nothing.
            // It still closes every order it names.
            self.finalize(&block.covers);
            return;
        }

        // `covers` arrives in whatever order the venue happened to build it, and
        // the outcome must not depend on that.
        eligible.sort();

        // Quantity above the basis belongs to orders this engine does not track.
        let divisible = if block.block_qty < basis {
            block.block_qty
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

        // Truncation leaves `r` base units unassigned, with `0 <= r < |E|`. The
        // first `r` orders in the ordering above take one each.
        let remainder = divisible - assigned;
        if remainder > 0 {
            for share in shares.iter_mut().take(remainder as usize) {
                *share += 1;
            }
        }

        for (entry, share) in eligible.iter().zip(shares) {
            self.submissions[entry.index].block_filled += share;
        }

        // A block is final for every order it names, whatever it gave them.
        self.finalize(&block.covers);
    }

    /// A block closes every order it names, whether or not it allocated anything
    /// to it: the venue cancelled whatever part of the order it did not fill.
    fn finalize(&mut self, covers: &[OrderId]) {
        for order_id in covers {
            if let Some(index) = self.resolve_cover(order_id) {
                self.submissions[index].finalized = true;
            }
        }
    }

    /// Fold the live order list into the ledger.
    ///
    /// Entries carry no match timestamp: `open_orders` is point-in-time truth
    /// about what is live right now, so the attribution window does not apply to
    /// it and an identifier that is live is still the order this engine tracked.
    /// It also supplies a `venue_seq` for an order the engine had not been told
    /// one for.
    pub fn absorb_open_orders(&mut self, open_orders: &[OpenOrder]) {
        for open_order in open_orders {
            let Some(index) = self.locate(&open_order.client_ref) else {
                continue;
            };
            let record = &mut self.submissions[index];
            if record.venue_seq.is_none() {
                record.venue_seq = Some(open_order.venue_seq);
            }
            // The list reports the same running total a fill event does, seen
            // from the other side. It is one figure about one order, so it
            // replaces rather than adds.
            if open_order.filled_qty > record.direct_filled {
                record.direct_filled = open_order.filled_qty;
            }
            if record.order_id.is_none() {
                record.order_id = Some(open_order.order_id.clone());
                self.index_by_order_id
                    .insert(open_order.order_id.clone(), index);
            }
        }
    }
}
