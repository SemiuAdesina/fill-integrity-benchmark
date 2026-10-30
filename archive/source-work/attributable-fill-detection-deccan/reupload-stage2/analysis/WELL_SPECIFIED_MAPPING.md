# Verifier case → public rule mapping (final package)

Files: VS = /app/spec/venue-semantics.md, RC = /app/spec/reconciliation-contract.md,
FC = /app/spec/frozen-contract.md. Every graded observation is the raw record of venue orders,
submission attempts, cancellations, positions, pending orders, cycle errors and inventory after a
scenario, compared byte for byte with the reference's record by a judge that never links
candidate code; nothing inspects source.

| case | public rule(s) | file / section | observable behaviour |
|---|---|---|---|
| v1, v2 | paired fills complete a mandate at the acquired sizes | RC §Attribution per order; instruction (service) | one position per mandate at leg sizes, no corrective order |
| v3 | one-legged fill corrected once, residual size | RC §Residual, §Closure | one corrective order of the leg size |
| v4 | cumulative events redelivered add nothing | VS §poll, RC §Attribution progress | one corrective, one position, acquired ≤ intended |
| v5a, v5b, v11b | custody inventory is not a fill | VS §instrument_inventory, RC §restart | no position/corrective from inventory; progress on a real fill |
| v6, v16 | residual only; list alone reports progress | RC §Residual; VS §open_orders | corrective of 12 for a leg of 30 filled 18 |
| v7 | stale/repeated observations fabricate nothing; frozen source | VS §poll, §inventory | no action in the first half, correct remainder in the second |
| v8 | completion from the feed alone | VS §poll | position from events with inventory frozen |
| v9 | lost acknowledgement retried under the same reference | RC §Submission outcomes; VS §submit | one economic corrective, one distinct client ref |
| v10 | shared instrument attributed per order | RC §Attribution per order | only the filled mandate is corrected/completed |
| v11, v17 | restart does not redo settled work; attribution durable | RC §restart | one corrective after two restarts; corrective sized from durable attribution |
| v12 | mandates independent | RC §Submission outcomes ("must not be latched shut") | two correctives, two positions |
| v13, v13b | safety and liveness under 32 seeded interleavings; determinism | RC §Determinism and all of the above | acquired ≤ intended per leg; repeatable records |
| v14 | refusal leaves the exposure open for a later attempt | RC §Submission outcomes | exactly one venue corrective after a refusal |
| v15 | unconfirmed leg never neutralized | RC §Visibility | no corrective for a leg that filled while the feed was withheld |
| v18, v18b, v19, v19b | half-open window at both ends, block match timestamp | VS §window, §blocks | position vs second corrective at ±1 s of each boundary |
| v20, v21, v22 | division ordering: window start, venue_seq, order_id; truncation; remainder one unit each | VS §How a block divides | follow-up corrective sizes 3/2/7 etc. |
| v23 | block_ref idempotency | VS §blocks | residual correctives once after three deliveries |
| v24, v24b | basis is current outstanding, not size | VS §division step 1–3; RC §Attribution | second block over residuals; exact division over outstanding |
| v25, v25b | compound: all of the above at once | VS §division; RC §finality | three positions vs two, one unit apart |
| v26 | identifier recycling; client-ref identity; closure state keyed by id does not outlive the mandate; restart | VS §window (recycling), RC §identity, §restart (final paragraph) | later mandate completes; one corrective; one cancel |
| v27 | lost acknowledgement on an entry leg: same reference, original window, durable across restart; unconfirmed rule | RC §Submission outcomes (AcknowledgementLost), §restart bullets; VS §submit | one economic order per leg (attempts: accepted, lost, deduplicated), fill attributed after release, other leg closed and corrected, one position |
| v28 | finality of every covered order incl. zero share; outstanding basis; block_ref after restart | VS §blocks, §division; RC §Finality, §restart | follow-ups 1, 1, 12; no double count after replay |
| v29 | fill event stamped at window end not attributed | VS §window | no position, no corrective, both legs still pending |
| v30 | unacknowledged corrective counts at full size while unresolved; retry under same reference twice | RC §Residual (third term), §Submission outcomes | one venue corrective, one distinct client ref, one position |
| 16 frozen probes | frozen constants and arithmetic | FC §Value types | raw values of cancel lead, exit window, position costs, exact quantity predicates, proportional share, window constant, timestamp arithmetic, cursor, config defaults |

Hidden semantic rule: none. Each of v26, v27, v28, v30 composes rules that are stated in the two
normative documents; the two clarifying sentences added to RC (a block-closed leg is withdrawn,
not unconfirmed; the lost-acknowledgement retry applies to every submission and keeps the first
attempt's window) state consequences of rules that were already present and are exercised
publicly (v27, v30 hidden; the block-closed leg by a public diagnostic only).
