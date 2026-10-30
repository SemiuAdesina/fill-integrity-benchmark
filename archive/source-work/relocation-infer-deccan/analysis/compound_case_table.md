# Compound case table (native screening against certified answers; verifier reward in the report)

| CASE | PUBLIC RULES | ENGINEERING PURPOSE | ORACLE A | ORACLE B | 01 reference | 02 solver B | 03 local fit | 04 contiguous | 05 floor div | 06 adjusted split | 07 prefers signed | 08 site-section B | battery variants failing here | redundancy | RETAIN |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| public | all six public types; every rule at once | the corpus a solver develops against; also graded | agrees | agrees | pass | pass | wrong | no output | no output | no output | wrong | no output | 21 (b01, b02, b03, b04, b05, b06, b07, b09, b10, b11, b12, b14, b15, b17, b18, b19, b21, b22, b23, b25, b32) | unique failures: 0 variants fail only here | RETAIN |
| case_a | EC §4 P/B definitions; RP §5 global consistency; expression inference | two objects consistent with S+A-P, the third forces S+B+A-P; P-only and B-only relinks | agrees | agrees | pass | pass | wrong | pass | no output | pass | pass | no output | 12 (b01, b03, b04, b06, b07, b09, b10, b14, b17, b19, b27, b32) | unique failures: 1 variants fail only here | RETAIN |
| case_b | RS §2 truncation; RS §3 signed ranges; RS §5 logical order + preserved bits; integer arithmetic | negative non-multiples of the scale across three reversed slices, addresses above 2^33, stale field bits | agrees | agrees | pass | pass | pass | no output | no output | pass | wrong | no output | 16 (b01, b02, b03, b04, b05, b06, b07, b09, b10, b13, b14, b15, b17, b18, b19, b32) | unique failures: 0 variants fail only here | RETAIN |
| case_c | EC §5 pair identity and first-record place; RS §2 Euclidean split; RP §3 shared expression/scale/split | carry boundaries and negative values in two pair groups (split 16 and 10, scale 1 and 4), lower type id lo in one group, hi in the other | agrees | agrees | pass | pass | wrong | pass | no output | no output | wrong | no output | 13 (b01, b02, b03, b05, b10, b11, b12, b19, b21, b22, b23, b24, b25) | unique failures: 1 variants fail only here | RETAIN |
| case_d | RP §4–§5 canonical key over genuine equivalence classes | A always 0 (S vs S+A), never negative (unsigned vs signed), never out of range (reject vs wrap), wrap (signedness unobservable) | agrees | agrees | pass | pass | wrong | pass | pass | pass | wrong | no output | 8 (b01, b03, b04, b06, b14, b17, b19, b32) | unique failures: 0 variants fail only here | RETAIN |
| case_e | RS §3 inclusive endpoints; RS §4 reject → null / wrap → modulo; EC §6 failure records | values at hi, hi+1, lo, lo-1 for signed/unsigned reject and wrap types | agrees | agrees | pass | pass | wrong | pass | pass | pass | wrong | no output | 10 (b01, b02, b03, b04, b06, b10, b14, b17, b19, b32) | unique failures: 0 variants fail only here | RETAIN |
| case_f | RP §5 per-type consistency with interleaved types; RF §2–§3 ordering; EC §2 64-bit containers | five types in two dense objects: a pair, a three-slice type, a 64-bit container, a wrap type | agrees | agrees | pass | pass | pass | no output | no output | no output | wrong | no output | 19 (b01, b03, b04, b05, b07, b09, b10, b11, b12, b13, b15, b17, b18, b19, b21, b22, b23, b25, b32) | unique failures: 0 variants fail only here | RETAIN |

## Snapshot progression (native, per fixture)

| snapshot | public | case_a | case_b | case_c | case_d | case_e | case_f |
|---|---|---|---|---|---|---|---|
| S0_starter | wrong | wrong | wrong | wrong | wrong | wrong | wrong |
| S1_scalar_replay | wrong | wrong | wrong | wrong | wrong | wrong | wrong |
| S2_first_inference | wrong | wrong | wrong | wrong | wrong | wrong | wrong |
| S3_fields | wrong | wrong | pass | wrong | wrong | wrong | wrong |
| S4_cross_object | wrong | pass | pass | wrong | pass | pass | wrong |
| S5_pairs | pass | pass | pass | pass | pass | pass | pass |
| S6_canonical_no_failure_records | wrong | wrong | wrong | wrong | wrong | wrong | wrong |
| S7_final | pass | pass | pass | pass | pass | pass | pass |

## Battery per fixture (native)

| variant | public | case_a | case_b | case_c | case_d | case_e | case_f |
|---|---|---|---|---|---|---|---|
| b01_place_without_base | no output | no output | no output | no output | no output | no output | no output |
| b02_object_base_for_b | no output | pass | no output | no output | pass | no output | pass |
| b03_symbol_index_off_by_one | no output | no output | no output | no output | no output | no output | no output |
| b04_rel_style_addend_ignored | no output | no output | no output | pass | no output | no output | no output |
| b05_addend_after_scale | no output | pass | no output | no output | pass | pass | no output |
| b06_never_considers_b | no output | no output | no output | pass | no output | no output | pass |
| b07_place_after_scale | no output | no output | no output | pass | pass | pass | no output |
| b09_scale_operands_first | no output | no output | no output | pass | pass | pass | no output |
| b10_exclusive_upper_bound | no output | no output | no output | no output | pass | no output | no output |
| b11_32bit_intermediate | no output | pass | pass | no output | pass | pass | no output |
| b12_truncating_remainder_split | no output | pass | pass | no output | pass | pass | no output |
| b13_wrap_before_division | pass | pass | wrong | pass | pass | pass | wrong |
| b14_signed_lower_bound_off_by_one | wrong | wrong | no output | pass | no output | no output | pass |
| b15_slices_sorted_by_lsb | wrong | pass | wrong | pass | pass | pass | wrong |
| b17_non_field_bits_zeroed | wrong | wrong | wrong | pass | wrong | wrong | wrong |
| b18_reversed_logical_order | wrong | pass | wrong | pass | pass | pass | wrong |
| b19_no_width_semantics | no output | no output | no output | no output | no output | no output | no output |
| b20_overlapping_slices_allowed | pass | pass | pass | pass | pass | pass | pass |
| b21_truncating_pair_quotient | no output | pass | pass | no output | pass | pass | no output |
| b22_members_fitted_independently | no output | pass | pass | no output | pass | pass | no output |
| b23_low_half_from_magnitude | no output | pass | pass | no output | pass | pass | no output |
| b24_split_then_scale | pass | pass | pass | no output | pass | pass | pass |
| b25_high_member_place | no output | pass | pass | no output | pass | pass | no output |
| b27_majority_vote_across_objects | pass | wrong | pass | pass | pass | pass | pass |
| b28_first_fit_in_enumeration_order | pass | pass | pass | pass | pass | pass | pass |
| b31_unsorted_output | pass | pass | pass | pass | pass | pass | pass |
| b32_rejection_as_zero | wrong | wrong | wrong | pass | wrong | wrong | wrong |
| solver_b | pass | pass | pass | pass | pass | pass | pass |
| p03_local_fit_first_object | wrong | wrong | pass | wrong | wrong | wrong | pass |
| p04_contiguous_field_only | no output | pass | no output | pass | pass | pass | no output |
| p05_floor_division | no output | no output | no output | no output | pass | pass | no output |
| p06_adjusted_split | no output | pass | pass | no output | pass | pass | no output |
| p07_prefers_signed | wrong | pass | wrong | wrong | wrong | wrong | wrong |
| p08_site_section_base | no output | no output | no output | no output | no output | no output | no output |
