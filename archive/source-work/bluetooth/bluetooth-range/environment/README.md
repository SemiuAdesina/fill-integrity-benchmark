
# Public feedback ladder

Run `cargo test --offline --locked --test diagnostics`. The starter is
intentionally incomplete, so several synthetic public checks initially fail
with rule-specific messages. These inputs are not verifier fixtures.

| Semantic family | Public executable diagnostic | Hidden graded coverage | Load-bearing |
|---|---|---|---|
| Signed half-open wrap | `signed_wrap_owns_negative_half_turn` | boundary + compound | yes |
| Exact RTT order | `rtt_rounds_propagation_before_adding_offsets` | RTT + compound | yes |
| Exact large-integer phase rounding | `large_integer_phase_rounding_stays_exact` | numeric boundary | yes |
| Joint PBR/RTT fit | `pbr_breaks_an_rtt_distance_clock_ambiguity` | joint + ambiguity | yes |
| Per-path biases | `antenna_paths_keep_independent_biases` | path + compound | yes |
| Full residual tuple | `maximum_residual_breaks_a_sum_tie` | non-exact compound | yes |
| Canonical tie | `empty_evidence_uses_the_canonical_parameter_tie` | tie cases | yes |
| Derived ordered predictions | `predictions_are_model_derived_and_canonically_ordered` | prediction + order | yes |
| Invalid inputs | `invalid_distance_and_path_grids_fail_closed` | negative/missing/empty | yes |
