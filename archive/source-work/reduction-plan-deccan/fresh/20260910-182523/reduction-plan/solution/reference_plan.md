# Reference plan

The reference recovery (`fixed/src/recover.cpp`) follows the specification
section by section, as a product of per-source paths.

1. Validity (problem-format-v1.md §3): positive payload and rates, non-negative
   latencies and capacities, int32 values, unique ids, disjoint node kinds,
   links between known distinct nodes.
2. Enumerate every simple path from each source's node to the sink; take the
   product in ascending source id; keep the suffix-consistent combinations
   (fabric-replay-v1.md §2) and, for each, every subset of the
   reduction-capable switches on the tree.
3. Replay each plan with a recursive bottom-up fold: links serve payloads in
   (ready tick, smallest origin) order, occupy ceil(bytes/rate) ticks and
   deliver after the latency (§3); selected switches need two payloads and
   capacity, reserve one payload of accumulator and emit one payload (§4); the
   sink folds all values in ascending origin id with saturation (§5).
4. Keep the plans that reproduce the telemetry (plan-recovery-v1.md §2) and
   take the minimum of (completion, allocation, total bytes, plan_key) (§3).

Invalidity short-circuits to the invalid document; an empty match set to the
infeasible document.
