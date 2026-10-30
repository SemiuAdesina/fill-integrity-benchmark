# Long-horizon analysis

## What the old runs looked like
Every substantive run: read spec + instruction + diagnostics (≈10–25 turns), rewrite
fill_classifier.rs and cycle.rs in one pass, build, run the shipped diagnostics (which named each
fix), write own tests, finish. Repair cycles after a failing run: 0–3 per slot, mostly on the
agent's own test scaffolding rather than on graded behaviour (`POSTMORTEM.md`). No run changed a
graded outcome after its first compiling build (`SNAPSHOT_MATRIX.txt`).

## Candidate-visible changes
- instruction.md: goal, spec locations, edit surface, build and diagnostic commands, deliverable,
  offline restrictions; 436 words; no rule list, no "what has to be true", no diagnosis of which
  concern is wrong.
- spec: two clarifications in reconciliation-contract.md (block-closed leg is withdrawn; lost
  acknowledgement retry applies to every submission with the first attempt's window) and one in
  venue-semantics.md (identifiers are recycled onto the engine's own later orders too); the
  source-map lost its eleven decision points; frozen-contract.md reworded for the new diagnostics.
- starter: unchanged bytes under src/ apart from the removed reproducer binary. Its defects are
  now spread across five concerns the redesigned contract exercises: observation reading, block
  division, closure decision, submission outcomes (entry legs), and identifier-keyed durable state.
- diagnostics: six independent suites, 24 scenarios, driving `Engine::run_cycle` against an
  in-memory venue; each assertion reports the observed outcome in contract terms and never the fix.
  On the starter 9/24 pass with failures in all six suites.

## Intermediate-snapshot matrix (public diagnostics, groups = obs/direct/block/neutr/restart/cross)
| slot | S0 starter | S1 first impl (ledger only) | S2 first compiling | S3 diagnostics green | S4 first semantic correction | S5 near-final | S6 final |
|---|---|---|---|---|---|---|---|
| 01 | 1/2/0/2/2/2 | 2/4/0/3/2/2 | = S6 | step 12 | – | – | 3/5/3/6/4/3 (hidden: fails v27) |
| 02 | same | 2/4/0/3/2/2 | step 32: 3/4/3/4/4/3 | never | – | = S2 | 3/4/3/4/4/3 (obs, direct, neutr failures persist to the end) |
| 03 | same | 2/4/0/3/2/2 | step 28 = green | step 28 | – | – | all green (hidden: fails v26, v27) |
| 04 | same | 2/3/0/2/2/2 → 2/4/0/3/2/2 | step 26: 2/5/3/5/4/3 | never | – | = S2 | identity + block-closed leg failures persist |
| 05 | same | 2/4/0/3/2/2 | step 27 = green | step 27 | – | – | green (hidden: fails v26, v27) |
| 06 | same | 2/4/0/3/2/2 | step 20 = green | step 20 | – | – | green (hidden: fails v26, v27) |
| 07 | same | 2/4/0/3/2/2 | step 16: 3/5/3/5/4/3 | never | – | = S2 | block-closed leg failure persists (hidden: passes) |
| 08 | same | 2/4/0/3/2/2 | step 32 = green | step 32 | – | – | green (hidden: passes) |

Reading: under the redesigned diagnostics, three of the eight historical runs (02, 04, 07) would
have finished with visible failures they never saw, each naming a distinct observable defect
(over-neutralization of an unconfirmed leg; a lost-acknowledgement recovery that never completes;
a venue-closed leg left uncovered). The five runs that went green in one pass did so because the
old instruction enumerated the consequences; the hidden suite then still fails four of them (v26,
v27), each with a failure a careful reading of the two documents predicts.

## Genuine run/observe/repair cycles a fresh strong agent should meet
1. First implementation from the two documents alone: the starter-shaped rewrite typically
   fixes the window, the high-water mark and the division, and the block suite goes green while
   observation_identity and neutralization_lifecycle still report failures (lost-ack recovery,
   recycled identifier, working quantity, unconfirmed leg) — a real diagnose-and-repair cycle
   that the old package pre-empted with its list.
2. The block-closed resting leg (public diagnostic): every design that reads the three-state
   rule literally sees "the residual of 9 base units was corrected as ([], [])" and has to
   reconcile finality with visibility (07 and 04 never did).
3. Identity: `a_recycled_identifier_belongs_to_the_order_that_holds_it_now` fails any
   identifier-first design and any design that binds identifiers from out-of-window observations.
4. Hidden: the lost entry acknowledgement across a restart requires new durable state and an entry
   retry path; the recycled identifier with stale closure state requires settlement cleanup. Both
   are reachable only by reading the submission-outcome and restart sections, not from any
   diagnostic, and both failed six of the eight historical finals.

## Risk
LONG-HORIZON RISK: MEDIUM. Structural evidence: the one-pass checklist is gone; the public
diagnostics produce staged failures across six independent groups on the starter and on the
first-pass snapshots of every historical run; three historical finals would have kept failing
publicly; and the hidden suite adds two lifecycle axes with no public counterpart. No numeric
median is claimed. The residual risk is that a very strong agent still writes a complete ledger
in one pass from the documents alone, as 07 and 08 nearly did; nothing legitimate remains to
lengthen that path without publishing the hidden compounds or adding artificial stages.
