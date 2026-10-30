# Compound case table (native screening against certified answers)

| CASE | separations verified at generation | 01 ref | 02 B | 03 lat-occ | 04 per-src | 05 no-serial | 06 comb-all | 07 sum-clamp | 08 bytes-first | battery failing here | unique |
|---|---|---|---|---|---|---|---|---|---|---|---|
| h_allocation_before_bytes | allocation_after_bytes | pass | pass | pass | wrong | pass | wrong | pass | wrong | 4 (b03, b04, b06, b16) | 0 |
| h_combine_choice | never_combine, combine_all_visited | pass | pass | pass | wrong | wrong | wrong | pass | pass | 6 (b03, b05, b09, b16, b20, b21) | 1 |
| h_infeasible_bytes | - | pass | pass | pass | pass | pass | pass | pass | pass | 1 (b15) | 0 |
| h_infeasible_completion | - | pass | pass | pass | pass | pass | pass | pass | pass | 2 (b05, b15) | 0 |
| h_invalid_duplicate_link | - | pass | pass | pass | pass | pass | pass | pass | pass | 2 (b13, b14) | 1 |
| h_invalid_negative_latency | - | pass | pass | pass | pass | pass | pass | pass | pass | 2 (b11, b14) | 1 |
| h_invalid_sink_is_endpoint | - | pass | pass | pass | pass | pass | pass | pass | pass | 2 (b12, b14) | 1 |
| h_invalid_unknown_node | - | pass | pass | pass | pass | pass | pass | pass | pass | 1 (b14) | 0 |
| h_no_completion | bytes_before_completion | pass | pass | wrong | wrong | pass | wrong | pass | wrong | 3 (b04, b06, b16) | 0 |
| h_pipelined_latency | latency_in_occupancy | pass | pass | wrong | wrong | pass | wrong | pass | wrong | 2 (b05, b06) | 0 |
| h_plan_key_canonical | key_by_input_order | pass | pass | pass | pass | pass | wrong | pass | pass | 2 (b08, b16) | 0 |
| h_saturation_fold | sum_then_clamp, fold_in_input_order | pass | pass | wrong | wrong | wrong | wrong | wrong | wrong | 6 (b03, b06, b10, b16, b20, b22) | 2 |
| h_serialization_shared_link | no_serialization | pass | pass | wrong | wrong | wrong | wrong | pass | pass | 1 (b16) | 0 |
| h_shared_endpoint | no_serialization | pass | pass | wrong | wrong | wrong | pass | pass | pass | 3 (b09, b16, b19) | 1 |
| h_suffix_tree | per_source_shortest | pass | pass | pass | wrong | pass | wrong | pass | pass | 3 (b08, b16, b20) | 0 |
| h_two_required_capacity | capacity_ignored | pass | pass | pass | wrong | pass | wrong | pass | pass | 2 (b01, b16) | 1 |
| h_zero_bytes_link | zero_telemetry_ignored | pass | pass | pass | wrong | pass | wrong | pass | pass | 2 (b04, b16) | 0 |
| public | per_source_shortest, latency_in_occupancy | pass | pass | wrong | wrong | pass | wrong | pass | wrong | 3 (b06, b16, b20) | 0 |

## Battery and snapshots (native, wrong fixtures)

- b01_capacity_ignored: 1 wrong (two_required_capacity)
- b02_one_payload_combine_allowed: 0 wrong — benign by construction
- b03_suffix_not_enforced: 3 wrong (allocation_before_bytes, combine_choice, saturation_fold)
- b04_zero_telemetry_ignored: 3 wrong (allocation_before_bytes, no_completion, zero_bytes_link)
- b05_completion_ignored: 3 wrong (combine_choice, infeasible_completion, pipelined_latency)
- b06_bytes_first_objective: 5 wrong (allocation_before_bytes, no_completion, pipelined_latency, saturation_fold, public)
- b07_key_by_link_id_only: 0 wrong — benign by construction
- b08_key_input_order: 2 wrong (plan_key_canonical, suffix_tree)
- b09_never_combine: 2 wrong (combine_choice, shared_endpoint)
- b10_fold_in_document_order: 1 wrong (saturation_fold)
- b11_rate_zero_accepted: 1 wrong (invalid_negative_latency)
- b12_sink_kind_not_checked: 1 wrong (invalid_sink_is_endpoint)
- b13_duplicate_link_ids_accepted: 1 wrong (invalid_duplicate_link)
- b14_invalid_reported_as_infeasible: 4 wrong (invalid_duplicate_link, invalid_negative_latency, invalid_sink_is_endpoint, invalid_unknown_node)
- b15_infeasible_reported_as_invalid: 2 wrong (infeasible_bytes, infeasible_completion)
- b16_paths_in_document_order: 11 wrong (allocation_before_bytes, combine_choice, no_completion, plan_key_canonical, saturation_fold, serialization_shared_link, shared_endpoint, suffix_tree, two_required_capacity, zero_bytes_link, public)
- b17_tie_by_document_position: 0 wrong — benign by construction
- b18_non_simple_paths_allowed: 0 wrong — benign by construction
- b19_unvisited_switch_selectable: 1 wrong (shared_endpoint)
- b20_occupancy_floor: 4 wrong (combine_choice, saturation_fold, suffix_tree, public)
- b21_allocation_counts_payloads: 1 wrong (combine_choice)
- b22_sink_value_from_arrival_order: 1 wrong (saturation_fold)
- S0_starter: 16 wrong (allocation_before_bytes, combine_choice, invalid_duplicate_link, invalid_negative_latency, invalid_sink_is_endpoint, invalid_unknown_node, no_completion, pipelined_latency, plan_key_canonical, saturation_fold, serialization_shared_link, shared_endpoint, suffix_tree, two_required_capacity, zero_bytes_link, public)
- S1_replay_without_serialization: 13 wrong (allocation_before_bytes, combine_choice, infeasible_bytes, infeasible_completion, no_completion, pipelined_latency, saturation_fold, serialization_shared_link, shared_endpoint, suffix_tree, two_required_capacity, zero_bytes_link, public)
- S2_serialized_replay: 13 wrong (allocation_before_bytes, combine_choice, infeasible_bytes, infeasible_completion, no_completion, pipelined_latency, saturation_fold, serialization_shared_link, shared_endpoint, suffix_tree, two_required_capacity, zero_bytes_link, public)
- S3_per_source_routes_no_combine: 13 wrong (allocation_before_bytes, combine_choice, infeasible_bytes, infeasible_completion, no_completion, pipelined_latency, saturation_fold, serialization_shared_link, shared_endpoint, suffix_tree, two_required_capacity, zero_bytes_link, public)
- S4_combine_search: 13 wrong (allocation_before_bytes, combine_choice, infeasible_bytes, infeasible_completion, no_completion, pipelined_latency, saturation_fold, serialization_shared_link, shared_endpoint, suffix_tree, two_required_capacity, zero_bytes_link, public)
- S5_telemetry_filter: 1 wrong (suffix_tree)
- S6_objective_without_key: 1 wrong (suffix_tree)
- S7_final: 0 wrong
