# Fixture certification table (author-side; generation-and-rejection)

| fixture | status | oracle A | oracle B | agree | replays | legal | matching | A s | B s | budget | separations verified | sources/switches/links |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| h_allocation_before_bytes | ok | ok | ok | True | 1380 | 290 | 42 | 0.009 | 0.006 | True | allocation_after_bytes | 3/3/17 |
| h_combine_choice | ok | ok | ok | True | 1054 | 351 | 6 | 0.007 | 0.006 | True | never_combine, combine_all_visited | 3/3/16 |
| h_infeasible_bytes | infeasible | infeasible | infeasible | True | 120 | 78 | 0 | 0.001 | 0.001 | True | - | 3/2/11 |
| h_infeasible_completion | infeasible | infeasible | infeasible | True | 396 | 146 | 0 | 0.003 | 0.002 | True | - | 3/3/15 |
| h_invalid_duplicate_link | invalid | invalid | invalid | True | None | None | None | 0.0 | 0.0 | True | - | 2/1/5 |
| h_invalid_negative_latency | invalid | invalid | invalid | True | None | None | None | 0.0 | 0.0 | True | - | 3/3/16 |
| h_invalid_sink_is_endpoint | invalid | invalid | invalid | True | None | None | None | 0.0 | 0.0 | True | - | 2/1/4 |
| h_invalid_unknown_node | invalid | invalid | invalid | True | None | None | None | 0.0 | 0.0 | True | - | 3/2/12 |
| h_no_completion | ok | ok | ok | True | 120 | 78 | 2 | 0.001 | 0.001 | True | bytes_before_completion | 3/2/11 |
| h_pipelined_latency | ok | ok | ok | True | 43 | 37 | 4 | 0.0 | 0.001 | True | latency_in_occupancy | 3/2/10 |
| h_plan_key_canonical | ok | ok | ok | True | 108 | 81 | 81 | 0.001 | 0.001 | True | key_by_input_order | 2/1/12 |
| h_saturation_fold | ok | ok | ok | True | 180 | 146 | 2 | 0.001 | 0.002 | True | sum_then_clamp, fold_in_input_order | 4/2/12 |
| h_serialization_shared_link | ok | ok | ok | True | 62 | 32 | 4 | 0.001 | 0.0 | True | no_serialization | 4/2/11 |
| h_shared_endpoint | ok | ok | ok | True | 166 | 88 | 2 | 0.002 | 0.001 | True | no_serialization | 3/3/15 |
| h_suffix_tree | ok | ok | ok | True | 341 | 95 | 2 | 0.002 | 0.001 | True | per_source_shortest | 3/3/15 |
| h_two_required_capacity | ok | ok | ok | True | 1236 | 376 | 9 | 0.007 | 0.006 | True | capacity_ignored | 3/3/17 |
| h_zero_bytes_link | ok | ok | ok | True | 456 | 196 | 3 | 0.003 | 0.003 | True | zero_telemetry_ignored | 3/3/16 |
| public | ok | ok | ok | True | 11916 | 5613 | 4 | 0.125 | 0.106 | True | per_source_shortest, latency_in_occupancy | 4/4/24 |

Every fixture: oracle A (path product, recursive replay) and oracle B (parent-pointer trees, event-driven replay) agree on status, plan, sink value and observables; both under the 20 s budget. Every trap fixture is retained only when the wrong-model family it targets yields a different answer. Rejected attempts during construction: h_pipelined_latency (11), h_serialization_shared_link (3), h_two_required_capacity (18), h_combine_choice (1), h_zero_bytes_link (1), h_shared_endpoint (2) — all for a missing separation or a search too small.

Unobservable by construction, therefore not graded as traps: the tie rule (ready tick, smallest origin id) cannot change link bytes, completion or the sink value when all payloads have the same size (the set of departure ticks on a link is permutation-invariant); the two-payload rule and the sink's non-allocation only affect legality of a candidate's own output, which the judge enforces on every written plan.
