# Long-horizon evidence

Old median 69.5 (16,57,60,60,79,82,88,93). Snapshot replay (analysis/snapshot_graded_matrix.txt) shows every
substantive attempt reached its final graded score at its first compiling build: 03 at step 9, 05 and 08 at
step 12, 01/02/06/07 at 69/70 on their first build (steps 19-24). Later turns: own test suites, sanitizer
sandbox flakiness, review. No graded outcome changed after the first build in any attempt.

Candidate-visible changes:
- instruction.md: rewritten to goal, contract location, edit surface, build/diagnostic commands, deliverable
  (no failure-mode list, no design hints; the contract lives only in spec/protocol.md).
- starter: unchanged.
- public diagnostics: /app/diagnostics/{runtime_smoke,lifecycle,ticket_identity,nested_coroutines,
  scheduler_clock}.cpp, CMake target `diagnostics`; each scenario stops at its first failed check and
  reports the observed fact in the words of the spec; nothing reveals a fix; groups are not gated.
- public specs: unchanged.

Intermediate-snapshot matrix with the diagnostics (lab/diag_snap, summarised):
snapshot | smoke | lifecycle | identity | nested | scheduler
starter | 5/5 | 1/6 | 1/4 | 0/4 | 1/5
01 first build (step 19) .. final | 5/5 | 5/6 then 4/6 | 4/4 | 4/4 | 5/5   (cancel-by-resume never fixed)
02 first build (step 21) .. final | 5/5 | 5/6 | 4/4 | 4/4 | 5/5
06 first build (step 19) .. final | 5/5 | 5/6 | 4/4 | 4/4 | 5/5
07 first build (step 24) .. final | 5/5 | 4/6 | 4/4 | 4/4 | 5/5
03 step 9 .. final | all pass
05 step 12, 20, 40 | all pass; step 39 (broken edit): 4/5, 6/6, 1/4, 3/4, 5/5, repaired at step 40
08 step 12 .. final | all pass

Genuine new run/observe/repair cycles the old intermediate snapshots would have encountered:
- 01, 02, 06, 07: the lifecycle group fails at the first build and keeps failing to the end (cancelled
  job's body runs again / settle count), a persistent, actionable signal none of them had; fixing it is a
  structural change (destroy instead of resume with reap at a quiet point), typically several cycles.
- 05 at step 39: the diagnostics would have shown the broken edit immediately (identity 1/4, nested 3/4).
- 03, 05, 08 at their first build: no diagnostic fails; the hidden N family separates 05 but is not a
  public signal.

Assessment: staged discovery is demonstrated for the four weaker designs and for the one mid-run regression;
the three strongest first-pass designs receive no new public failure, so their turn counts stay governed by
self-verification. LONG-HORIZON RISK: MEDIUM-HIGH (see final report for the rating and its basis).
