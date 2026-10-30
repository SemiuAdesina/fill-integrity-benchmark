# Reference plan

The reference recovery (`fixed/src/recover.cpp`) follows the specification
section by section, as an exact branch-and-bound over suffix-consistent
trees. Graded fabrics are far beyond enumeration (plan-recovery-v1.md §4), so
the search prunes, and only with a bound that never discards a plan that
could be minimal.

1. Validity (problem-format-v1.md §3): positive payload and rates, non-negative
   latencies and capacities, int32 values, unique ids, disjoint node kinds,
   links between known distinct nodes. Listed bytes that are not a whole
   number of payloads, or a source that cannot reach the sink, are
   infeasible at once; links listed with 0 bytes are removed before the
   search, since no plan that uses one reproduces the telemetry.
2. Route the sources one at a time, the one with the largest unloaded cost
   to the sink first, each along every simple path that stays off the
   partial tree until it joins it (fabric-replay-v1.md §2). Abandon a partial
   tree when its bound exceeds the listed completion, or the best completion
   found so far: the bound is the partial tree replayed for the routed
   sources with every capable switch that gathers two payloads selected
   (adding a source never lowers completion, selecting a switch never raises
   it), plus the unloaded shortest cost of every source not yet routed. Abandon
   it also when a listed link can no longer be reproduced: its tail already
   leaves on another link, no unrouted source can still reach its tail off
   the tree, or its payload count is out of reach.
3. Replay each complete tree with every subset of the capable switches it
   visits, after a payload-count filter against the listed links, with a
   recursive bottom-up fold: links serve payloads in (ready tick, smallest
   origin) order, occupy ceil(bytes/rate) ticks and deliver after the
   latency (§3); selected switches need two payloads and capacity, reserve
   one payload of accumulator and emit one payload (§4); the sink folds all
   values in ascending origin id with saturation (§5).
4. Keep the plans that reproduce the telemetry (plan-recovery-v1.md §2) and
   take the minimum of (completion, allocation, total bytes, plan_key) (§3),
   comparing the full plan key explicitly so no tie is decided by search
   order.

Invalidity short-circuits to the invalid document; an empty match set to the
infeasible document.
