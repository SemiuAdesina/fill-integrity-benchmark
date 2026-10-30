# Verifier check → public rule matrix (author-side)

Every graded behaviour maps to an explicit public rule. No verifier-only semantics.

| VERIFIER CHECK | PUBLIC RULE | SPEC FILE | SECTION | OBSERVABLE BEHAVIOUR |
|---|---|---|---|---|
| result_documents_well_formed | document members, one program per manifest type ascending, one prediction per site per link in order, program grammar and normal form, pair member agreement | result-format-v1.md; relocation-program-v1.md | RF §1–§5; RP §1–§3 | the document parses strictly, validates against the manifest, and is rejected on any violation |
| predictions_follow_submitted_programs | predictions are derived values equal to the replay of the submitted programs | result-format-v1.md; replay-semantics-v1.md | RF §3; RS §1–§5 | verifier replay of the submitted programs equals every submitted word/null |
| predictions_match_legacy_outcomes | a fitting program reproduces every legacy word, acceptance and rejection; predictions for replay links are the legacy linker's words | relocation-program-v1.md; elf-corpus-v1.md; replay-semantics-v1.md | RP §5; EC §3, §6; RS §4, §6 | every prediction equals the recorded/withheld legacy outcome, null exactly at rejected sites |
| hidden_perturbations_replay | one program describes a type everywhere; replay is defined for any link map | relocation-program-v1.md; elf-corpus-v1.md | RP §5; EC §3–§4 | submitted programs replayed on verifier-owned link maps reproduce the legacy words |
| scalar_expression_and_scale | expression set, truncating division by the scale | relocation-program-v1.md; replay-semantics-v1.md | RP §1; RS §2 | recovered expression and scale equal the certified ones for ordinary types |
| section_base_versus_place | definitions of P (site section base + r_offset) and B (symbol's section base, 0 for absolute) | elf-corpus-v1.md | EC §4 | the type consistent with S+A-P on two objects is recovered as S+B+A-P from the third |
| negative_truncation_and_split_fields | truncation toward zero, signed ranges, slices in logical order with preserved bits, integer arithmetic without machine width | replay-semantics-v1.md; relocation-program-v1.md | RS §2–§5; RP §2 | programs and hidden words of case_b (addresses above 2^33, negative non-multiples of the scale, reversed slices) |
| pair_coupling_and_carry | pair instance identification, first record's place, Euclidean split, shared expression/scale/split, per-member rejection | elf-corpus-v1.md; replay-semantics-v1.md; relocation-program-v1.md | EC §5; RS §1–§2, §4; RP §3 | pair programs equal certified ones; hidden pair words at carry boundaries and negative values reproduce, either record first, scale 4 |
| canonical_equivalence | the required answer is the fitting program with the smallest published key | relocation-program-v1.md | RP §4–§5 | for types with several equivalent fits the minimum-key program is submitted |
| reject_wrap_boundaries | inclusive range endpoints, reject → null, wrap → Euclidean modulo | replay-semantics-v1.md | RS §3–§4 | programs of case_e and hidden words/nulls at hi, hi+1, lo, lo-1 |
| field_layout_recovery | slice semantics, normal form, logical vs physical order | relocation-program-v1.md; replay-semantics-v1.md | RP §2; RS §5 | multi-slice programs (including physically reversed ones) equal certified ones |
| cross_object_consistency | one program per type across every site, object and link | relocation-program-v1.md | RP §5 | programs recovered from objects carrying several interleaved types equal certified ones |
| deterministic_output | byte-identical output on repeated runs; canonical ordering and numeric formatting | result-format-v1.md; instruction | RF §3–§4; instruction "What is required" 4 | two runs over the public corpus produce identical documents |
| negative_control_must_fail | (verifier-owned control; not a candidate rule) | — | — | a deliberately perturbed expectation is never matched |

Section keys: RF = result-format-v1.md, RP = relocation-program-v1.md, RS = replay-semantics-v1.md, EC = elf-corpus-v1.md.

Conventions that are public and load-bearing (no hidden convention):
- P = `bases[section of the site] + r_offset` (EC §4).
- B = link base of the symbol's defining section; `0` for `SHN_ABS` and for undefined symbols resolved with `section: null` (EC §4).
- S = `st_value` for section-defined and absolute symbols; the manifest `value` for undefined symbols (EC §4).
- Pair instance = consecutive RELA entries of one pair group, same symbol index, same addend, either order; the first entry's place is the pair's P (EC §5).
- Split: `lo = Q mod 2^k` (Euclidean), `hi = (Q - lo) / 2^k` (RS §2); rejection per member (RS §4).
- Canonical key order (RP §4); joint key for pairs = key of the lower type number followed by the other (RP §4).
- Normal form: contiguous ascending slices are merged (RP §2).
- Failed links: every listed site rejected, every unlisted site accepted, no words (EC §6).
