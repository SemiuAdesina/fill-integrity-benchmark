# Compound case table (native screening against certified answers)

| CASE | PUBLIC RULES | PURPOSE | oracle A | oracle B | 01 ref | 02 B | 03 rot | 04 phys | 05 equal | 06 tie | 07 resize | 08 block-cov | battery failing here | unique |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| public | every rule at once | the inputs a solver develops against; also graded | agree | agree | pass | pass | wrong | wrong | pass | pass | wrong | pass | 14 (b01, b02, b03, b04, b08, b09, b12, b14, b15, b17, b18, b19, b20, b23) | 0 |
| h_topology | CF §1–§2, RS §1, §7 | PP 2→4 and TP 2→3 with unaligned cuts; partial-block records from several shards | agree | agree | pass | pass | wrong | wrong | pass | pass | wrong | pass | 12 (b01, b02, b03, b09, b12, b14, b15, b17, b18, b19, b20, b23) | 0 |
| h_tied_replicas | RS §1, §4, §5 | head.weight restored solely from embed.weight records; three replicas with rings of different lengths | agree | agree | pass | pass | wrong | wrong | wrong | wrong | wrong | pass | 14 (b01, b02, b03, b04, b08, b12, b14, b15, b17, b18, b19, b20, b23, b25) | 1 |
| h_roles_resize | RS §1, §8, §9 | forward/backward distinct; targets of length 1, 2, 12, 16 | agree | agree | pass | pass | wrong | wrong | pass | pass | wrong | pass | 13 (b01, b02, b03, b04, b08, b12, b14, b15, b17, b18, b19, b20, b23) | 0 |
| h_older_samples_differ | RS §4 | longer ring differs only in samples older than the shorter ring: ACCEPT | agree | agree | pass | pass | wrong | wrong | wrong | pass | wrong | pass | 12 (b01, b02, b03, b04, b08, b12, b14, b15, b17, b19, b20, b23) | 0 |
| h_inactive_gap_accept | RS §6, §7 | coverage gap only under an inactive target: ACCEPT (the same layout with the target active rejects) | agree | agree | pass | pass | wrong | wrong | pass | pass | wrong | pass | 12 (b01, b02, b03, b09, b10, b12, b14, b15, b17, b19, b20, b23) | 1 |
| h_boundaries | CF §1, RS §6, §8 | block size 1000, lengths 2001 and 3000, cut points around block edges | agree | agree | pass | pass | wrong | wrong | pass | pass | wrong | pass | 10 (b01, b02, b03, b12, b14, b15, b17, b19, b20, b23) | 0 |
| h_ordering | RS §10 | bytewise tensor names, stages 2/10/11, shards 0/9 | agree | agree | pass | pass | wrong | wrong | pass | pass | wrong | pass | 12 (b01, b02, b03, b12, b14, b15, b16, b17, b18, b19, b20, b23) | 1 |
| h_reject_scale_unneeded | RS §2, §4 | scale disagreement in a block no active target needs | agree | agree | pass | pass | pass | pass | pass | pass | pass | pass | 1 (b06) | 1 |
| h_reject_suffix | RS §4 | newest sample of a uniquely-long ring differs | agree | agree | pass | pass | pass | pass | pass | pass | pass | pass | 1 (b24) | 1 |
| h_reject_cursor | RS §2 | cursor equal to the ring length | agree | agree | pass | pass | pass | pass | pass | pass | pass | pass | 2 (b07, b22) | 1 |
| h_reject_empty_ring | RS §2 | an empty ring | agree | agree | pass | pass | pass | pass | pass | pass | pass | pass | 1 (b22) | 0 |
| h_reject_coverage_gap | RS §7 | a one-index hole inside a block that is otherwise described | agree | agree | pass | pass | pass | pass | pass | pass | pass | wrong | 1 (b21) | 1 |
| h_reject_duplicate_targets | RS §6 | two targets overlapping by one index | agree | agree | pass | pass | pass | pass | pass | pass | pass | pass | 1 (b11) | 1 |
| h_reject_zero_target_length | RS §6 | history_length 0 on an inactive target | agree | agree | pass | pass | pass | pass | pass | pass | pass | pass | 1 (b13) | 1 |

## Snapshots (native, per fixture)

| snapshot | public | topology | tied_replicas | roles_resize | older_samples_differ | inactive_gap_accept | boundaries | ordering | reject_scale_unneeded | reject_suffix | reject_cursor | reject_empty_ring | reject_coverage_gap | reject_duplicate_targets | reject_zero_target_length |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| S0_starter | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| S1_parse_and_emit | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong |
| S2_rotation | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong |
| S3_consistency_and_selection | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | wrong | wrong | wrong | wrong | wrong |
| S4_record_and_target_validation | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | wrong | pass | pass |
| S5_index_coverage | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| S6_resizing | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| S7_final | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |

## Battery (native, per fixture)

| variant | public | topology | tied_replicas | roles_resize | older_samples_differ | inactive_gap_accept | boundaries | ordering | reject_scale_unneeded | reject_suffix | reject_cursor | reject_empty_ring | reject_coverage_gap | reject_duplicate_targets | reject_zero_target_length |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| b01_rotation_off_by_one_backward | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| b02_ring_taken_as_stored | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| b03_roles_merged | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| b04_shortest_history_chosen | wrong | pass | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| b05_tiebreak_tensor_before_replica | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| b06_scale_check_skipped | pass | pass | pass | pass | pass | pass | pass | pass | wrong | pass | pass | pass | pass | pass | pass |
| b07_cursor_equal_to_length_allowed | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | wrong | pass | pass | pass | pass |
| b08_suffix_compares_oldest | wrong | pass | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| b09_inactive_targets_emitted | wrong | wrong | pass | pass | pass | wrong | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| b10_inactive_targets_need_coverage | pass | pass | pass | pass | pass | wrong | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| b11_overlap_not_detected | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | wrong | pass |
| b12_duplicate_ignores_role | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| b13_zero_length_only_checked_when_active | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | wrong |
| b14_last_block_end_inclusive | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| b15_order_backward_first | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| b16_stage_compared_as_string | pass | pass | pass | pass | pass | pass | pass | wrong | pass | pass | pass | pass | pass | pass | pass |
| b17_cursor_after_padding | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| b18_scale_recomputed_from_history | wrong | wrong | wrong | wrong | pass | pass | pass | wrong | pass | pass | pass | pass | pass | pass | pass |
| b19_pad_with_oldest_sample | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| b20_unsorted_output | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| b21_partial_states_on_gap | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | wrong | pass | pass |
| b22_record_validation_skipped | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | wrong | wrong | pass | pass | pass |
| b23_touching_intervals_rejected | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| b24_suffix_only_between_equal_lengths | pass | pass | pass | pass | pass | pass | pass | pass | pass | wrong | pass | pass | pass | pass | pass |
| b25_coverage_ignores_tie_partner_records | pass | pass | wrong | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| solver_b | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| p03_rotation_from_newest | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| p04_physical_identity | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| p05_full_ring_equality | pass | pass | wrong | pass | wrong | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| p06_identity_ignores_tie_group | pass | pass | wrong | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| p07_resize_keeps_oldest | wrong | wrong | wrong | wrong | wrong | wrong | wrong | wrong | pass | pass | pass | pass | pass | pass | pass |
| p08_block_level_coverage | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | wrong | pass | pass |
