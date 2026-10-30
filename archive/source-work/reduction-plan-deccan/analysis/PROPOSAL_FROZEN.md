# Accepted proposal (frozen at grant; transcribed verbatim from the hub on 2026-09-10)

Title: Recover Canonical In-Network Reduction Plans from Scale-Up Fabric
Category: ML · Subcategory: Distributed Training Infrastructure · 4 h expert time · granted 9 Sept 2026 · revision 5 · grant ACTIVE (one immutable ZIP)
Proposal id 4db5f161457411d4346c05f84db6d5b9; raw sha256 307c4fcd…b71

Recover the in-network reduction plan that produced a collective trace after an accelerator-fabric topology, firmware or routing change. /app/problem.json describes endpoint sources, a distinct sink, directed links with integer rate/latency, reduction-capable switches with accumulator capacity, equal-size int32 payloads, observed link bytes and optionally completion. /app/result.json returns ok with source-id-ordered paths and ascending combine ids, invalid for malformed ids/topology, non-positive rate/payload or negative latency, or infeasible when no legal plan matches.

A payload ready at t starts a link at max(t,link_free), occupies it for ceil(bytes/rate) ticks, then arrives after latency; ties serialize by (ready_tick,smallest_origin_id). Paths are simple; after two paths meet, their remaining edges to the sink are identical. At a selected switch, every payload reaching that node participates once after the last arrives; at least two are required. Reduction has zero latency, emits one payload-sized message, unions origins and folds original int32 values in ascending origin id with saturation after each addition. It reserves one payload-sized accumulator, legal when allocation <= capacity. Selected unvisited switches are illegal. The sink is the final reducer, is not selectable, and completion is when its last required payload arrives. Link bytes sum transmitted payload sizes; unused links contribute zero.

Legal plans must reproduce all telemetry. Minimize (completion,total accumulator allocation,total link bytes,plan_key) lexicographically; plan_key is source-id-ordered flattened (from_node,link_id) paths followed by ascending combine ids. The artifact lets engineers compare pre/post-change behavior, explain fabric hot spots or lost switch offload, and reproduce the routing decision. The verifier replays the plan; graded captures are kept only when two independent exact searches certify the same canonical optimum within budget.

## Reviewer notes (Stage 0, revision 5)
- Verifiable Accept; Well-specified UNCERTAIN; Solvable Accept; Difficult Accept; Interesting Accept; Outcome-verified Strong Accept.
- "The proposal does not yet make every JSON representation and boundary condition explicit" → every schema and boundary condition must be public and exact.
- Client-fit: concrete industrial purpose; prose direct and outcome-oriented; the model passage is dense but acceptable.
