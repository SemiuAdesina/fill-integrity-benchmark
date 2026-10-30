#!/usr/bin/env python3
"""Partial-fix battery: each variant is the reference with one targeted change."""
import os, shutil, sys
R=os.path.expanduser("~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2")
REF=f"{R}/lab/ref2/src"; OUT=f"{R}/lab/battery"
FC="engine/fill_classifier.rs"; CY="engine/cycle.rs"
V = {}
def variant(name, *edits):
    V[name]=edits
variant("v01_add_cumulative_fills", (FC, """        if event.cumulative_filled_qty > record.direct_filled {
            record.direct_filled = event.cumulative_filled_qty;
        }""", """        record.direct_filled += event.cumulative_filled_qty;"""))
variant("v02_separate_event_and_list_totals", (FC, """            if open_order.filled_qty > record.direct_filled {
                record.direct_filled = open_order.filled_qty;
            }""", """            record.block_filled += open_order.filled_qty;"""))
variant("v03_inclusive_window_end", (FC, "at >= record.window_start && at < record.window_start + ATTRIBUTION_WINDOW", "at >= record.window_start && at <= record.window_start + ATTRIBUTION_WINDOW"))
variant("v04_venue_id_only_identity", (FC, """        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            return Some(index);
        }
        self.index_of_order_at(order_id, at)""", """        let _ = client_ref;
        self.index_of_order_at(order_id, at)"""))
variant("v05_client_ref_only_without_window", (FC, """        if !Self::in_window(&self.submissions[index], event.at) {
            return;
        }
        self.bind_order_id(index, &event.order_id);""", """        self.bind_order_id(index, &event.order_id);"""))
variant("v06_block_replay_counted_again", (FC, """        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }
""", """        self.absorbed_blocks.insert(block.block_ref.clone());
"""))
variant("v07_equal_split_instead_of_proportional", (FC, """            let share = proportional_share(divisible, *outstanding, basis);""", """            let _ = outstanding;
            let share = divisible / eligible.len() as Qty;"""))
variant("v08_basis_uses_original_size", (FC, """            let outstanding = record.outstanding();
            if outstanding <= 0 {
                continue;
            }""", """            let outstanding = record.size;
            if record.outstanding() <= 0 {
                continue;
            }"""))
variant("v09_drops_integer_remainder", (FC, """        let mut remainder = divisible - assigned;
        for share in shares.iter_mut() {
            if remainder <= 0 {
                break;
            }
            *share += 1;
            remainder -= 1;
        }""", """        let _ = assigned;"""))
variant("v10_remainder_to_largest_order", (FC, """        let mut remainder = divisible - assigned;
        for share in shares.iter_mut() {
            if remainder <= 0 {
                break;
            }
            *share += 1;
            remainder -= 1;
        }""", """        let remainder = divisible - assigned;
        if remainder > 0 {
            let mut largest = 0usize;
            for (position, (_, _, outstanding)) in eligible.iter().enumerate() {
                if *outstanding > eligible[largest].2 {
                    largest = position;
                }
            }
            shares[largest] += remainder;
        }"""))
variant("v11_ordering_by_identifier_only", (FC, """        eligible.sort_by(|left, right| left.0.cmp(&right.0));""", """        eligible.sort_by(|left, right| left.0 .2.cmp(&right.0 .2));"""))
variant("v12_zero_share_orders_not_finalized", (FC, """        for ((_, index, _), share) in eligible.iter().zip(shares) {
            self.submissions[*index].block_filled += share;
        }

        self.finalize(&covered);""", """        let mut closed: Vec<usize> = Vec::new();
        for ((_, index, _), share) in eligible.iter().zip(shares) {
            self.submissions[*index].block_filled += share;
            if share > 0 {
                closed.push(*index);
            }
        }

        self.finalize(&closed);"""))
variant("v13_neutralizes_full_leg_size", (CY, "let gap = leg.size - self.acquired_for(leg) - self.outstanding_for(leg);", "let gap = leg.size;"))
variant("v14_ignores_attributed_quantity", (CY, "let gap = leg.size - self.acquired_for(leg) - self.outstanding_for(leg);", "let gap = leg.size - self.outstanding_for(leg);"))
variant("v15_ignores_working_neutralizations", (CY, "let gap = leg.size - self.acquired_for(leg) - self.outstanding_for(leg);", "let gap = leg.size - self.acquired_for(leg);"))
variant("v16_lost_ack_treated_as_rejection", (CY, """            Err(SubmitError::AcknowledgementLost) => {
                self.ledger.track(client_ref.clone(), None, None, size, now);
            }
            Err(_) => {
                self.neutralizations.retain(|slot| slot.client_ref != client_ref);
            }""", """            Err(_) => {
                self.neutralizations.retain(|slot| slot.client_ref != client_ref);
            }"""))
variant("v17_rejection_treated_as_committed", (CY, """            Err(_) => {
                self.neutralizations.retain(|slot| slot.client_ref != client_ref);
            }
        }

        Ok(())""", """            Err(_) => {}
        }

        Ok(())"""))
variant("v18_direct_progress_not_persisted", (CY, "            submissions: self.ledger.submissions(),", "            submissions: self.ledger.submissions().into_iter().map(|mut r| { r.direct_filled = 0; r }).collect(),"))
variant("v19_block_refs_not_persisted", (CY, "            absorbed_blocks: self.ledger.absorbed_blocks(),", "            absorbed_blocks: Vec::new(),"))
variant("v20_committed_neutralizations_not_persisted", (CY, "            neutralizations: self.neutralizations.clone(),", "            neutralizations: Vec::new(),"))
variant("v21_completion_from_custody_inventory", (CY, """    fn leg_is_complete(&self, leg: &PendingOrder) -> bool {
        self.acquired_for(leg) >= leg.size
    }""", """    fn leg_is_complete(&self, leg: &PendingOrder) -> bool {
        self.inventory
            .instrument_inventory(&leg.instrument_id)
            .map(|held| held >= leg.size)
            .unwrap_or(false)
    }"""))
variant("v22_attribution_shared_across_mandates", (CY, """    fn acquired_for(&self, leg: &PendingOrder) -> Qty {
        let mut total = self.ledger.attributed_for(&leg.client_ref);
        for record in self.records_for(leg) {
            total += self.ledger.attributed_for(&record.client_ref);
        }
        total
    }""", """    fn acquired_for(&self, leg: &PendingOrder) -> Qty {
        let mut total = 0;
        for order in self.pending.all() {
            if order.instrument_id == leg.instrument_id && order.venue == leg.venue {
                total += self.ledger.attributed_for(&order.client_ref);
            }
        }
        for record in &self.neutralizations {
            if record.instrument_id == leg.instrument_id && record.venue == leg.venue {
                total += self.ledger.attributed_for(&record.client_ref);
            }
        }
        total
    }"""))
variant("v23_entry_lost_ack_dropped", (CY, """            Err(SubmitError::AcknowledgementLost) => {
                self.entry_attempts.push(EntryAttempt {""", """            Err(SubmitError::Transport(_)) => {
                self.entry_attempts.push(EntryAttempt {"""))
variant("v24_block_closed_leg_left_unconfirmed", (CY, "        } else if self.replaced.contains(&leg.order_id) || self.ledger.is_finalized(&leg.client_ref) {", "        } else if self.replaced.contains(&leg.order_id) {"))
variant("v25_finalized_orders_stay_eligible", (FC, """            if record.finalized || !Self::in_window(record, block.at) {
                continue;
            }""", """            if !Self::in_window(record, block.at) {
                continue;
            }"""))
variant("v26_closure_state_not_cleared_on_completion", (CY, """        for order_id in [&north_leg.order_id, &south_leg.order_id] {
            self.pending.remove(order_id);
            self.closing.remove(order_id);
            self.replaced.remove(order_id);
        }""", """        for order_id in [&north_leg.order_id, &south_leg.order_id] {
            self.pending.remove(order_id);
        }"""))
variant("v27_venue_id_resolved_before_client_ref", (FC, """        if let Some(index) = self.index_by_client_ref.get(client_ref).copied() {
            return Some(index);
        }
        self.index_of_order_at(order_id, at)""", """        if let Some(index) = self.submissions.iter().position(|record| record.order_id.as_ref() == Some(order_id)) {
            return Some(index);
        }
        let _ = at;
        self.index_by_client_ref.get(client_ref).copied()"""))
variant("v28_client_sequence_reset_on_restart", (CY, "            self.client_seq = snapshot.client_seq;", "            self.client_seq = 0;"))
variant("v29_unacknowledged_neutralization_counts_nothing", (CY, """                None => total += record.size,""", """                None => {
                    let _ = record.size;
                }"""))
variant("v30_lost_ack_retried_under_fresh_reference", (CY, """        for record in unconfirmed {
            let request = Self::neutralization_request(&record);""", """        for mut record in unconfirmed {
            let fresh = self.next_client_ref();
            if let Some(slot) = self.neutralizations.iter_mut().find(|slot| slot.client_ref == record.client_ref) {
                slot.client_ref = fresh.clone();
            }
            record.client_ref = fresh;
            let request = Self::neutralization_request(&record);"""))
variant("v31_unconfirmed_leg_neutralized", (CY, "            LegVisibility::Unconfirmed => return Ok(()),\n            LegVisibility::Resting => {", "            LegVisibility::Resting => {"), (CY, "            LegVisibility::Withdrawn => {}\n        }\n\n        let gap", "            LegVisibility::Withdrawn | LegVisibility::Unconfirmed => {}\n        }\n\n        let gap"))
variant("v32_finalized_neutralization_still_working", (FC, """        match self.record(client_ref) {
            Some(record) if record.finalized => 0,
            Some(record) => record.outstanding(),
            None => 0,
        }""", """        match self.record(client_ref) {
            Some(record) => record.outstanding(),
            None => 0,
        }"""))
names = sys.argv[1:] or list(V)
for name in names:
    d=f"{OUT}/{name}/src"; shutil.rmtree(f"{OUT}/{name}", ignore_errors=True); shutil.copytree(REF, d)
    for (f, old, new) in V[name]:
        p=f"{d}/{f}"; s=open(p).read()
        assert old in s, f"{name}: anchor not found in {f}: {old[:70]!r}"
        s=s.replace(old, new, 1); open(p,"w").write(s)
    print("wrote", name)
print(len(names), "variants")
