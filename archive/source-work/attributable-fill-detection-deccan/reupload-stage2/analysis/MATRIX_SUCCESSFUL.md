# Successful implementations 01/03/05/06/07/08 against the reference and the public contract

Evidence: reconstructed final code (`replay/slotXX/final/src`), the lab probes (`lab/probe/`), the
final graded replay (`analysis/final/replay/RESULTS.txt`) and the public-diagnostic runs
(`analysis/diag_finals.log`). "old ref" is the v10 reference; "new ref" the re-upload reference.

| behaviour | public rule | 01 | 03 | 05 | 06 | 07 | 08 | old ref | new ref |
|---|---|---|---|---|---|---|---|---|---|
| A. event cumulative high-water | RC §Attribution progress | yes | yes | yes | yes | yes | yes | yes | yes |
| A. open-order cumulative high-water, one figure with the feed | VS §open_orders, RC | yes | yes | yes | yes | yes | yes | yes | yes |
| A. event then list / list then event / replay / lower stale value | same | yes | yes | yes | yes | yes | yes | yes | yes |
| A. exact window start / end, match timestamp | VS §window | yes | yes | yes | yes | yes | yes | yes | yes |
| A. recycled venue id on a fill event | VS §window (recycling), RC §identity | yes | yes | yes | yes | yes | yes | **no** (id first, oldest) | yes |
| A. client reference identity (lost-ack recovery) | RC §identity, §outcomes | yes | yes | yes | yes | yes | yes | yes | yes |
| A. restart after partial direct fill | RC §restart | yes | yes | yes | yes | yes | yes | yes | yes |
| B. block_ref dedup, before and after restart | VS §blocks, RC §restart | yes | yes | yes | yes | yes | yes | yes | yes |
| B. eligible set / outstanding basis / integer shares / remainder ranking | VS §division | yes | yes | yes | yes | yes | yes | yes | yes |
| B. zero-share covered order finalized | VS §blocks, RC §finality | yes | yes | yes | yes | yes | yes | yes | yes |
| B. finalized order excluded from a later block's basis | RC §finality | **no** (kept eligible) | yes | yes | yes | yes | yes | **no** | yes |
| B. block after partial direct fill / direct fill after block | RC §finality | yes | yes | yes | yes | yes | yes | yes | yes |
| B. block naming a recycled id resolved at the block instant | VS §window ("inside a window an identifier is unambiguous") | no | no | yes | no | no | no | no | yes (ungraded) |
| B. two mandates sharing an instrument in one block | RC §Attribution per order | yes | yes | yes | yes | yes | yes | yes | yes |
| C. residual only; subtract attributed; subtract working | RC §Residual | yes | yes | yes | yes | yes | yes | yes | yes |
| C. lost-ack neutralization counts full size; finalized counts nothing | RC §Residual | yes | yes | yes | yes | yes | yes | yes | yes |
| C. lost ack retried under same reference; rejection retried later | RC §Submission outcomes | yes | yes | yes | yes | yes | yes | yes | yes |
| C. unconfirmed leg never neutralized | RC §Visibility | yes | yes | yes | yes | yes | yes | yes | yes |
| C. block-closed resting leg corrected without a cancel | RC §Visibility (clarified) + §Finality | yes | yes | yes | yes | **no** | yes | **no** | yes (public diagnostic, ungraded) |
| C. later progress after a prior correction (working reopened when a corrective is finalized) | RC §Residual | yes | yes | yes | yes | yes | yes | yes | yes |
| D. cursor, window starts, direct high-water, block refs, committed neutralizations, client refs durable | RC §restart | yes | yes | yes | yes | yes | yes | yes | yes |
| D. lost-ack entry leg durable and retried under the same reference | RC §Submission outcomes (clarified), §restart | **no** | **no** | **no** | **no** | yes | yes | **no** | yes (graded v27) |
| D. restart between submission and acknowledgement (entry leg) | same | no | no | no | no | yes | yes | no | yes |
| E. same instrument in two mandates; block naming both; one settled one open | RC §Attribution per order | yes | yes | yes | yes | yes | yes | yes | yes |
| E. identifier-keyed closure state does not outlive the mandate | RC §restart (clarified) + VS recycling | yes | **no** | **no** | **no** | yes | yes | **no** | yes (graded v26) |

Key: yes = matches the public rule in the probes; no = observable deviation. Rows marked "ungraded"
are exercised by the public diagnostics or left ungraded on purpose (see FINAL_REPORT).

What 03/05/08 style designs got right that made v10 too easy: everything the seven named
readings covered, because the readings were named. What separates them now: the lost entry
acknowledgement (only 07 and 08 track and retry an unacknowledged entry leg with its original
window across a restart) and identifier-keyed closure state after settlement (01, 07, 08 clear it).
