# Candidate-visible hint audit (author-side)

| PUBLIC STATEMENT | CONTRACT REQUIRED? | IMPLEMENTATION CONSEQUENCE? | SOLUTION HINT? | DECISION |
|---|---|---|---|---|
| instruction: migration story, what the checkpoint holds, why wrong state is worse than no state | goal | none | no | KEEP |
| instruction: "rebuild … by logical identity rather than by where it happened to live, validate that the old placements agree, resize, write canonical form, or refuse" | yes (restates the deliverable in the proposal's own words) | none | no (what, not how) | KEEP |
| instruction: frozen/yours table, edit surface `src/restore.cpp` + `src/restore/` | yes | none | no | KEEP |
| instruction: "currently rejects every layout" | yes (starter state) | none | no | KEEP |
| instruction: spec list with one-line scope each; build/run/diag commands; paths | yes | none | no | KEEP |
| instruction: required-result items 1–4 | yes | none | no | KEEP |
| instruction: "graded on other checkpoints and layouts … any well-formed input" | yes (scope) | none | no | KEEP |
| CF §1: blocks, placements, tie groups explained | yes | defines identity | no (public rule) | KEEP |
| CF §2: record interval within one block; block = floor(begin/B) | yes | shapes the data model | no | KEEP |
| CF §4: well-formedness, malformed → reject | yes | none | no | KEEP |
| RS §2: global validation "whether or not a target needs it" | yes (edge convention made public per the reviewer's request) | none | no | KEEP |
| RS §3: rotation with the explanation "h[c] is the oldest because c is the next write" | yes | none | mild: explains the rule's rationale; the rule itself is the contract | KEEP |
| RS §4: suffix rule with "samples older than the shorter history's oldest sample are not compared" | yes | none | mild: names the edge; it is the rule, not the algorithm | KEEP |
| RS §5: "(by section 4 all longest histories are identical, so the choice cannot change the output …)" | yes (proposal's rule) | none | mild: states a consequence; no algorithm | KEEP |
| RS §6–§7: duplicates, inactive targets, index-level coverage, tie partner records count | yes | none | no | KEEP |
| RS §8–§10: resize, state fields, ordering | yes | none | no | KEEP |
| RS §11: "the reason for the rejection is not part of the graded output" | yes | none | no | KEEP |
| DS §1–§4: the integer model; "restoration copies [the scale pair]; it never recomputes them" | yes | none | mild: forecloses one wrong design; it is a stated rule (proposal: "scale pair" is a copied record field) | KEEP |
| RF §1–§4: document shape, reject document, determinism, writer supplied | yes | none | no | KEEP |
| starter restore.cpp comment: what is frozen, what is yours, "rejects every layout: it restores nothing" | yes | none | no | KEEP |
| model.hpp: `Record::group()`, `Record::block()`, `Target::first_block()/last_block()` helpers | scaffolding | encode CF §1–§2 arithmetic | mild: the block arithmetic is public; the helpers remove one class of off-by-one error but leave every semantic decision | KEEP |
| model.hpp: `state_before` (published order) and `write_result` | yes (published order and canonical serialisation must exist once) | a solver can call them | no | KEEP (frozen) |
| model.hpp: `step` (frozen model) | yes | lets a solver test states locally | no | KEEP (frozen) |
| diag groups: inputs, result_schema, placements, scale_pairs, history_shape, ordering, model_step | public tooling | name the areas a document can be wrong in | mild: mirror the spec's concepts; no rule or algorithm named | KEEP |
| diag placements message on a reject: "a rejection is only right when a rule of restoration-semantics-v1.md §2, §4, §6 or §7 applies" | symptom | points at the public rules | no | KEEP |
| diag history_shape: "nonzero sample does not occur in any record of that block"; "oldest samples exceed every record's length but are not zero" | symptom | reveals provenance and padding side | mild: RS §8 is public; the message names the observation, not the fix | KEEP |
| examples/small: a worked example with its restored document | format aid | shows rotation, resizing, an inactive target, both roles on one small case | mild: illustrates public rules on one example; hidden layouts differ in every trap | KEEP |

Absent by design: any "to solve this, do X then Y" text, prohibited-approach lists, TODO checklists, mention of search, oracles, hidden fixture names, or grading vocabulary. Terms scanned for and absent from candidate-visible files: Stage 2, Stage 3, rollout, 2/8, Claude, benchmark, pass rate, difficulty gate, latent, oracle, author-side.
