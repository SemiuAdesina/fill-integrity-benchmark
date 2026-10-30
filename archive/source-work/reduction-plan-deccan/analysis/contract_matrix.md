# Verifier check → public rule matrix (author-side)

| VERIFIER CHECK | PUBLIC RULE | SPEC FILE | SECTION | OBSERVABLE BEHAVIOUR |
|---|---|---|---|---|
| result_documents_well_formed | the three document shapes | result-format-v1.md | §1–§3 | every artifact parses strictly |
| invalid_problems_reported | validity conditions; validity decided first | problem-format-v1.md | §3 | duplicate link id, negative latency, a source on the sink node, an unknown node: the invalid document |
| infeasible_problems_reported | no legal plan reproduces the telemetry | plan-recovery-v1.md | §1–§2 | bytes that are not a whole number of payloads; a completion no plan reaches: the infeasible document |
| plans_legal_and_reproduce_telemetry | plan legality; telemetry reproduction; sink value is the replayed value | fabric-replay-v1.md; plan-recovery-v1.md; result-format-v1.md | FR §2, §4; PR §2; RF §3 | every written plan replays legally under the verifier's engine and matches the telemetry |
| canonical_plans_recovered_exactly | the unique minimum of (completion, allocation, bytes, plan_key) | plan-recovery-v1.md | §3 | paths, combine and sink value equal the certified canonical plan on every solvable fixture |
| pipelined_link_serialization | occupancy ceil(bytes/rate), link free after occupancy, arrival after latency; serialization on shared links; several sources on one endpoint | fabric-replay-v1.md | §1, §3 | three fixtures whose canonical plan differs under latency-in-occupancy or independent-link models |
| suffix_consistent_tree | paths simple and suffix-consistent | fabric-replay-v1.md | §2 | per-source fastest paths are not a tree; the canonical tree differs |
| capacity_and_combine_choice | one payload-sized accumulator, legal when ≤ capacity; subset choice of selected switches | fabric-replay-v1.md | §4 | a 7-byte-capacity switch cannot reduce 8-byte payloads; exactly one combine is right in the other fixture |
| saturating_fold_value | fold of original values in ascending origin id with saturation after each addition | fabric-replay-v1.md | §1, §5 | 2147483000 + 1000 saturates before −3000 is added; sum-then-clamp and document-order folds differ |
| telemetry_zero_links_and_optional_completion | a listed 0 pins a link unused; completion optional | plan-recovery-v1.md; problem-format-v1.md | PR §2; PF §1 | a zero-listed link excludes plans; without completion the objective decides |
| objective_order_and_plan_key | allocation before bytes; plan_key definition and comparison | plan-recovery-v1.md | §3 | fewer combines beats fewer bytes at equal completion; parallel equal links resolved by bytewise (from_node, link_id) |
| public_capture | all of the above on the public fabric | all | — | the public problem (11,916 candidate replays, 4 telemetry-matching plans) recovered exactly |
| graded_fabrics_recovered_within_budget | the answer is the objective's minimum over every legal telemetry-consistent plan, for fabrics whose plans cannot be enumerated within the budget; 120 s per problem | plan-recovery-v1.md | §1–§4 | six fabrics of 10–12 sources, 14 switches and 82–102 links (listed and absent completion, reductions pinned by telemetry, one infeasible); both exhaustive solvers exhaust the probe budget and the run fails closed; the reference answers each in under 2.4 seconds |
| deterministic_output | byte-identical output | result-format-v1.md | §4 | two runs over the public problem identical and exact |
| negative_control_must_fail | verifier-owned control | — | — | a perturbed expectation is never matched |

Rules stated in the proposal that are legality-only or unobservable under equal payload sizes, therefore enforced on candidate output (legality replay) rather than by recovery traps: the tie rule (ready tick, smallest origin id); the two-payload minimum at a selected switch; the sink being unselectable and reserving no accumulator; selected unvisited switches being illegal.
