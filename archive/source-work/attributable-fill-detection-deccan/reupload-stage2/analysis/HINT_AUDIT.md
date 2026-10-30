# Candidate-visible hint audit (original v10 package)

| public statement (where) | necessary contract? | implementation consequence? | solution hint? | decision |
|---|---|---|---|---|
| "Quantity counts toward a leg only when genuinely attributable … custody inventory … can never prove that one particular order filled" (instruction, What has to be true) | yes (spec: InventorySource) | yes: do not use inventory for completion | yes: names the starter's absent defect and the design | removed from instruction; stays in venue-semantics |
| "Direct fill progress is cumulative … high-water mark instead of adding quantity … block shares additive, idempotent by block_ref" | yes (reconciliation-contract §Attribution progress) | yes | yes: literally the two fixes | removed from instruction; stays in the spec |
| "half-open window … at the start is inside; at the end is outside" | yes (venue-semantics) | yes | yes: the `<`/`<=` fix | removed; stays in spec |
| "eligible set, basis of outstanding quantity, truncated shares, remainder in the deterministic order … finalized afterwards, including one allocated nothing" | yes | yes | yes: the division rule restated as a checklist | removed; stays in spec |
| "residual only — required, less attributed, less working … unknown state never neutralized … closure before corrective … lost ack same reference … rejection leaves exposure open" | yes | yes | yes: names close_gap, visibility and submission fixes | removed; stays in spec |
| "settled work stays settled … window starts, feed position, attribution, absorbed block references, tracked work, committed neutralizations survive" | yes | yes | partial | removed; stays in spec |
| "The structure is sound. The semantics are not … defects are in how observations are read and derived quantities are computed, not in how the state is stored" (instruction, The problem) | no | yes | yes: excludes whole concerns from suspicion | removed |
| "`fill_classifier.rs` holds one record per order … `cycle.rs` drives that ledger" (instruction) | no | yes | yes: names the two files to rewrite | removed; neutral file map kept in source-map |
| "Where a reading of the venue is chosen" — eleven decision points with what to check (source-map) | no | yes | yes: a file-by-file repair recipe | removed |
| "checks one documented rule at a time … reports the rule it broke" and "five whole failure modes" (instruction, source-map) | no | no | yes: tells the candidate the aids localise the defects | removed with the aids |
| `tests/diagnostics.rs` assertion messages naming the fix; direct calls on `AttributionLedger::{track,absorb_batch,attributed_for}` | no | yes | yes: fix list plus a prescribed internal API | replaced by six observable-outcome suites through `Engine::run_cycle` |
| `src/bin/reproduce.rs` five coarse scenarios with explanatory detail lines | no | partly | yes | removed |
| Starter comments ("Place them on the largest outstanding order, which has the most room") | no | no | no: plausible wrong justifications, realistic | kept |
| `frozen-contract.md` "Two of the diagnostics … read fields" | no | no | no | reworded |

Answers to the blind-view questions:
- Which difficult deductions were already stated? All of them: the instruction's list is the reference plan's fix list in prose.
- Which source comments identify the bugs? None; comments justify the wrong readings plausibly.
- Does source-map.md turn the task into a recipe? Yes, via the eleven "where a reading is chosen" points; removed.
- Does reproduce.rs name five complete failure modes? Yes (pre-existing inventory, shared instrument, stale feed, one-legged fill, restart); removed.
- Do public test names expose the consequences? Yes (`cumulative_fill_quantities_are_a_high_water_mark`, `block_remainder_units_go_to_the_first_orders_in_the_ordering`, `a_lost_acknowledgement_is_retried_under_the_same_client_ref`); replaced.
- Can an agent transform the starter mechanically from the prose? Yes; every run did.
- Does a correct first architecture solve almost everything? Yes; the starter's architecture was already the reference's, and the seven readings were enumerated.
