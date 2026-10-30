# Plan recovery, version 1

What `/app/result.json` must contain for a given problem.

## 1. Outcome

1. If the problem is invalid (`problem-format-v1.md`, section 3): the
   invalid document.
2. Otherwise consider every legal plan (`fabric-replay-v1.md`, section 2 and
   section 4) that **reproduces the telemetry** (section 2 below). If there is
   none: the infeasible document.
3. Otherwise: the ok document carrying the unique plan that is minimal under
   the objective of section 3, and its sink value.

## 2. Telemetry

A plan reproduces the telemetry when, after replay, the bytes of every link
listed in `telemetry.link_bytes` equal the listed value (a listed `0` means
the link carried nothing), and, when `telemetry.completion` is present, the
plan's completion equals it. Links not listed are unconstrained. Telemetry
never lists a link id that does not exist.

## 3. Objective

Plans are compared by the tuple

```
(completion, total accumulator allocation, total link bytes, plan_key)
```

lexicographically, smallest first. The **plan_key** of a plan is the list of
`(from_node, link_id)` pairs of every link of every path, paths taken in
ascending source id and links in path order, followed by the selected switch
ids in ascending order. Two plan_keys are compared lexicographically: pair by
pair, a pair by `from_node` then `link_id` (bytewise), and a list that is a
proper prefix of the other is smaller; if the path lists are equal, the
combine lists are compared the same way. Two distinct legal plans never
share a plan_key, so the minimum is unique.

## 4. Search space and budget

The set of legal plans is finite: every path is a simple path in the fabric
from the source's node to the sink, and every selected switch is a
reduction-capable switch on the tree. Graded fabrics are larger than the
public problem: up to 12 sources, 13 switches and about 100 links, with
hundreds to thousands of simple paths per source, so the legal plans of a
graded fabric cannot all be enumerated in the time allowed. A solver has 120
seconds of wall-clock time per problem on two cores, and 24 minutes for all
graded problems together; the reference recovers every graded fabric in
well under five seconds. A solver may prune as long as
it never discards a plan that could be minimal under section 3: the answer is
defined by sections 1 to 3 alone, never by what a bounded search happened to
visit. `/app/examples/scale_sample/` is a problem of graded size with the
document it produces.

## 5. Determinism

The same problem always yields the same document byte for byte
(`result-format-v1.md`, section 4).
