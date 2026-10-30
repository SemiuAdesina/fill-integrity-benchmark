# Candidate-visible hint audit (author-side)

Everything a fresh solver sees: instruction.md, spec/*, starter source, the two
binaries, comments, type names, diagnostic group names and messages.

| PUBLIC STATEMENT | CONTRACT REQUIRED? | IMPLEMENTATION CONSEQUENCE? | SOLUTION HINT? | DECISION |
|---|---|---|---|---|
| instruction: migration story, evidence description, deliverable | yes (goal) | none | no | KEEP |
| instruction: frozen/yours table, edit surface `src/recover.rs` + `src/recover/` | yes (edit surface) | none | no | KEEP |
| instruction: "the placeholder decodes the corpus and emits a placeholder document that recovers nothing" | yes (state of starter) | none | no | KEEP |
| instruction: four spec files listed with one-line scope each | yes | none | no | KEEP |
| instruction: build, run, diagnostic commands, paths | yes | none | no | KEEP |
| instruction: "What is required of the result" 1–4 (fit, min key, predictions, determinism) | yes (restates spec) | none | no (states what, not how) | KEEP |
| instruction: "graded on corpora other than the one supplied" | yes (scope) | none | no | KEEP |
| instruction: diagnostics "hold no answer … necessary, not sufficient" | yes (expectation setting) | none | no | KEEP |
| spec EC §4: P, S, B definitions | yes | defines operands | no (public rule) | KEEP |
| spec EC §5: pair instance rule, first record's place | yes | defines pairs | no | KEEP |
| spec EC §6: failed links list every rejected site, unlisted accepted | yes | rejection evidence | no | KEEP |
| spec RP §2: normal form (merge contiguous ascending slices) | yes | output form | no | KEEP |
| spec RP §4: canonical key, joint key | yes | tie rule | no | KEEP |
| spec RP §5: "with overflow = wrap the encoded bits do not depend on signedness" example | yes (illustrates equivalence) | none | mild: names one equivalence class; it is a mathematical consequence of §RS 4 and the key resolves it; a solver still has to find the fit set | KEEP (spec, not instruction) |
| spec RS §2–§5: arithmetic, split, ranges, overflow, encoding | yes | none | no | KEEP |
| spec RF: document, ordering, numbers, rejection conditions | yes | none | no | KEEP |
| starter recover.rs doc comment: "loader, ELF reader, schema types, canonical key and serialiser are frozen; this module is yours" | yes | none | no | KEEP |
| starter recover.rs body: iterate objects/links/sites, `word: None`, `result.sort()` | scaffolding | shows how to enumerate sites and sort | no (no inference) | KEEP |
| program.rs: `CanonicalKey` and `Program::key()` implementing the published order | yes (published key must be implemented once, correctly) | a solver can call it instead of re-implementing it | no (the key is public; the search is the work) | KEEP (frozen) |
| program.rs: `Program::validate`, `Recovery::validate` | yes (public rejection rules) | lets a solver check its own document | no | KEEP (frozen) |
| bits.rs: `place`, `extract`, `slice_mask`, `read_word`, `write_word` | helpers | encoding mechanics given | no (spec RS §5 defines them; nothing about which slices) | KEEP |
| corpus.rs: `Site` with raw fields (section, offset, type, symbol index, addend, container word), `Link::legacy_word`, `Link::rejected` | scaffolding | raw data access | no (operands S/P/B are not resolved; pairs not identified) | KEEP |
| diagnostic group names: corpus_integrity, result_schema, scalar_replay, field_replay, pair_replay, cross_object_consistency, replay_links | yes (public tooling) | names the areas a result can be wrong in | mild: the names mirror the spec's own concepts; no rule or algorithm is named | KEEP |
| diagnostic messages: "predictions modify bits {…} the legacy linker never modified"; "legacy modifies bits {…} no prediction modifies"; "sites differ only inside / outside the legacy-modified bits" | symptom | tells where, not why | no | KEEP |
| diagnostic messages: "instances whose first record is type N: a/b fully reproduced" | symptom | splits pair agreement by record order | mild: hints that record order matters; the rule (first record's place) is public in EC §5 | KEEP |
| diagnostic message: "predicted a word but the legacy linker rejected this site" / "predicted a rejection where the legacy linker accepted" | symptom | names rejection evidence | no (EC §6 is public) | KEEP |
| diagnostic context lines: symbol kind (absolute / undefined-resolved / defined in section), addend, site-section base | symptom context | shows the operand kinds involved in a mismatch | no (all are corpus facts) | KEEP |
| reference_plan.md, solve.sh, solution/fixed | not visible (solution/) | — | — | n/a (never in the agent image) |
| tests/ | not visible | — | — | n/a |

Removed or never written (compared with the earlier toy package): the "do not use X, Y, Z" prohibited-approach list; the diagnostic that names the failing semantic family; any TODO enumerating grammar dimensions; the comment "global route/… inference is the intended repair surface"-style pointer at an algorithm; any mention of a search, enumeration, candidate count, key-order enumeration, or oracle.

Terms scanned for and absent from candidate-visible files: Stage 2, Stage 3, rollout, 2/8, Claude, benchmark, pass rate, difficulty gate, latent, oracle, fixture names of hidden corpora.
