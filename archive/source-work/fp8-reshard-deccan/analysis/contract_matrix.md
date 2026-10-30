# Verifier check → public rule matrix (author-side)

Every graded behaviour maps to an explicit public rule. No verifier-only semantics, no hidden convention.

| VERIFIER CHECK | PUBLIC RULE | SPEC FILE | SECTION | OBSERVABLE BEHAVIOUR |
|---|---|---|---|---|
| result_documents_well_formed | document shape of the restored and reject documents; integer members; no unknown members | result-format-v1.md | §1–§3 | every artifact parses strictly |
| accept_layouts_restored_exactly | identity, rotation, consistency, selection, coverage, resizing, state fields, ordering | restoration-semantics-v1.md | §1, §3–§5, §7–§10 | states of every consistent layout equal the certified canonical states exactly |
| inconsistent_layouts_rejected | the reject conditions | restoration-semantics-v1.md; result-format-v1.md | RS §2, §4, §6, §7, §11; RF §2 | every inconsistent layout yields the reject document |
| next_state_trace_replay | the integer model consumes the restored state; ring order and cursor matter beyond the newest sample | delayed-scaling-model-v1.md | §1–§3 | verifier replay of hidden (amax, value) sequences from each written state equals the certified trace |
| topology_move_ragged_shards | placement is not identity; a record covers part of a block; several records describe a block; index-level coverage | checkpoint-format-v1.md; restoration-semantics-v1.md | CF §1–§2; RS §1, §7 | PP 2→4, TP 2→3 with unaligned cuts restored exactly |
| tied_tensors_and_replicas | identity by tie group; records of any tensor of the group; longest compatible history; suffix rule | restoration-semantics-v1.md | §1, §4, §5 | a tensor whose own records are absent is restored from its tied partner across three replicas of different ring lengths |
| role_separation_and_resizing | forward/backward distinct; shorter target keeps newest; longer target left-pads zeros; cursor 0 | restoration-semantics-v1.md | §1, §8, §9 | targets of length 1, 2, 12, 16 against rings of 3–6 samples, both roles |
| suffix_rule_older_samples | only the overlapping newest samples are compared | restoration-semantics-v1.md | §4 | a longer ring differing only in older samples is accepted; a newest-sample difference is rejected |
| coverage_and_inactive_targets | inactive targets need no coverage; every index of an active target must be covered | restoration-semantics-v1.md | §6, §7 | a gap under an inactive target accepts; a one-index hole under an active target rejects |
| global_record_validation | every record is validated and every identity is checked, needed or not | restoration-semantics-v1.md | §2, §4 | scale disagreement in an unneeded block, a cursor equal to the ring length, an empty ring: reject |
| target_validation | duplicate placements or overlapping intervals; zero history length on any target | restoration-semantics-v1.md | §6 | a one-index overlap and a zero length on an inactive target: reject |
| block_boundaries_and_ordering | block of index i = floor(i/B); last block short; bytewise tensor order; forward before backward; numeric stage/shard | checkpoint-format-v1.md; restoration-semantics-v1.md | CF §1; RS §6, §10 | block size 1000 with length 2001/3000; "Zeta" before "alpha", "layer.10" before "layer.2", stages 2/10/11, shards 0/9 |
| deterministic_output | byte-identical output; canonical serialisation | result-format-v1.md | §4 | two runs over the public inputs are identical and exact |
| negative_control_must_fail | (verifier-owned control) | — | — | a deliberately perturbed expectation is never matched |

Conventions that are public and load-bearing:
- Record interval lies in one block; the record's block is floor(logical_begin / B) (CF §2).
- Identity = (tie group or tensor, role, block) (RS §1).
- Chronological = h[c..], h[..c] (RS §3); newest sample at h[(c-1) mod L].
- Consistency and validation are global over the checkpoint (RS §2, §4).
- Coverage is index-level over records of the identity group and role, any tensor of the group (RS §7).
- Duplicates: same (tensor, role, stage, shard) or overlapping intervals of one tensor and role, active or not (RS §6).
- Resize: newest L kept / zeros on the old side; cursor 0 (RS §8).
- Order: tensor bytewise, forward < backward, stage, shard, block numeric (RS §10).
- Model: write at cursor, advance, peak ≥ 1, inverse = pow2ceil(peak), scale = 2^24/inverse, quantized = trunc(v·scale/2^24) saturated ±448 (DS §2).
