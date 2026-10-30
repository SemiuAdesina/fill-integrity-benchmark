# Fixture certification table (author-side; generation-and-rejection)

| fixture | expected | oracle A | oracle B | states | traces A=B | retained |
|---|---|---|---|---|---|---|
| h_boundaries | accept | accept | accept | 20 | yes | True |
| h_inactive_gap_accept | accept | accept | accept | 8 | yes | True |
| h_older_samples_differ | accept | accept | accept | 6 | yes | True |
| h_ordering | accept | accept | accept | 24 | yes | True |
| h_reject_coverage_gap | reject | reject:coverage | reject:coverage | - | yes | True |
| h_reject_cursor | reject | reject:record | reject:record | - | yes | True |
| h_reject_duplicate_targets | reject | reject:overlap | reject:overlap | - | yes | True |
| h_reject_empty_ring | reject | reject:record | reject:record | - | yes | True |
| h_reject_scale_unneeded | reject | reject:scale | reject:scale | - | yes | True |
| h_reject_suffix | reject | reject:suffix | reject:suffix | - | yes | True |
| h_reject_zero_target_length | reject | reject:zero target history | reject:zero target length | - | yes | True |
| h_roles_resize | accept | accept | accept | 32 | yes | True |
| h_tied_replicas | accept | accept | accept | 26 | yes | True |
| h_topology | accept | accept | accept | 50 | yes | True |
| public | accept | accept | accept | 27 | yes | True |

Rejected attempts during construction: h_older_samples_differ attempt 0 (the perturbed old sample fell inside an overlap: both oracles rejected, expected accept); h_inactive_gap_accept regenerated until the gap under the inactive target was verified to reject when the target is active.
