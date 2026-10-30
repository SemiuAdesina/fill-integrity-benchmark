# Candidate-visible hint audit (author-side)

| PUBLIC STATEMENT | CONTRACT REQUIRED? | IMPLEMENTATION CONSEQUENCE? | SOLUTION HINT? | DECISION |
|---|---|---|---|---|
| instruction: fabric offload story, what operators see, why they need the plan | goal | none | no | KEEP |
| instruction: "produce the plan ... that the fabric model reproduces exactly, choosing the canonical plan when several would, and reporting problems that are malformed or that no legal plan can explain" | yes (the proposal's deliverable) | none | no (what, not how) | KEEP |
| instruction: frozen/yours table, edit surface `src/recover.cpp` + `src/recover/` | yes | none | no | KEEP |
| instruction: "currently reports every problem as infeasible" | yes (starter state) | none | no | KEEP |
| instruction: spec list, commands, paths, required-result items 1–4, "graded on other fabrics" | yes | none | no | KEEP |
| PF §1–§2: document, node kinds | yes | data model | no | KEEP |
| PF §3: validity list and "validity is decided before anything else" | yes | fixes status precedence | no (rule) | KEEP |
| FR §1: payload value = fold of original values in ascending origin id, saturation after each addition, "origins never counted twice" | yes | none | mild: the last clause forecloses folding partial sums; it is the rule the proposal states | KEEP |
| FR §2: simple, suffix-consistent, "hence the links form a tree", selected switches capable, visited, sink never selectable | yes | describes the plan space | mild: names the tree consequence, which any reader derives | KEEP |
| FR §3: link serialization with occupancy and latency after it | yes | none | no | KEEP |
| FR §4: two payloads, capacity, one accumulator per selected switch, allocation formula | yes | none | no | KEEP |
| FR §5: sink as final reducer; "equivalently, the fold of all source values" | yes | none | mild: states the consequence that sink_value does not depend on the plan; it is a direct consequence of §1 | KEEP |
| FR §6: observables | yes | none | no | KEEP |
| PR §1–§3: outcome order, telemetry reproduction, objective, plan_key with comparison rule, uniqueness | yes | none | no | KEEP |
| PR §4 (v2): "the set of legal plans is finite ... graded fabrics are larger than the public problem: up to 12 sources, 13 switches and about 100 links, hundreds to thousands of simple paths per source, so the legal plans cannot all be enumerated in the time allowed; 120 seconds per problem on two cores; the reference recovers every graded fabric in well under five seconds; a solver may prune as long as it never discards a plan that could be minimal; the answer is defined by sections 1 to 3 alone" | solvability envelope and budget | tells the solver enumeration will not finish and that pruning must be sound | mild: names the constraint (exactness under a budget), not the bound, the search order, or the property that makes a bound sound | KEEP |
| instruction item 5 and `examples/scale_sample/`: a problem of graded size with its document | budget and size are part of the contract | the candidate can time a solver on a representative problem | mild: the sample's answer is one data point; the graded fabrics differ in every trap | KEEP |
| RF §1–§4: document shapes, rejection, determinism, writer supplied | yes | none | no | KEEP |
| starter comment: what is frozen, what is yours, "validates nothing, replays nothing and searches nothing" | yes | none | mild: names the three things missing, which are the three specification files | KEEP |
| model.hpp: types with document-order vectors; `Result::ok/invalid/infeasible` constructors | scaffolding | none | no | KEEP |
| diag groups: problem, result_schema, path_structure, selection, sink_value, telemetry_scope | tooling | reveal the structural rules of FR §2, §4, §5 as symptoms | mild: the rules are public; no replay, no search, no objective | KEEP |
| diag sink_value message: "not the fold of the source values in ascending id with saturation" | symptom of a public rule | none | mild | KEEP |
| examples: plain, switch_reduction, infeasible with their documents | format aid | shows a one-combine answer and an infeasible telemetry | mild: three tiny cases; hidden fabrics differ in every trap | KEEP |

Absent by design: any "enumerate paths, then subsets, then replay" text, any description of a bound (the monotonicity of completion under added sources or added reductions is never stated), prohibited-approach lists, TODO checklists, mention of search order, oracles, fixture names, or grading vocabulary. Terms scanned for and absent from candidate-visible files: Stage 2, Stage 3, rollout, 2/8, Claude, benchmark, pass rate, difficulty gate, latent, oracle, author-side.
