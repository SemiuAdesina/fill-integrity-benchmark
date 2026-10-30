# Fabric replay, version 1

The deterministic model of how a plan executes on the fabric. A plan is a
path per source and a set of selected switches; replaying it yields the
arrival ticks, the bytes each link carried, the accumulator allocation and the
sink value. Every rule here is graded through the result the solver writes.

## 1. Payloads

A payload has a set of **origins** (source ids), a size of `payload_bytes`,
and a **ready tick** at the node it is currently at. Each source's payload has
the single origin `{id}` and is ready at tick 0 at the source's node.

The **value** of a payload with origins `O` is the fold of the *original*
source values of `O` in ascending origin id: start with the value of the
smallest id, then for each further id in ascending order add its value and
saturate the sum to the int32 range `[-2^31, 2^31 - 1]` after each
addition. Origins are never counted twice: a payload's value depends only
on its origin set.

## 2. Plans

A plan gives, for every source in ascending source id, a **path**: a
sequence of link ids from the source's node to the sink, each link starting
at the node the previous one ended at. Paths are **simple**: no node is
visited twice by one path. Paths are **suffix-consistent**: once two paths
reach the same node, the rest of both paths, up to the sink, is the same
sequence of links. Hence the links used by a plan form a tree directed toward
the sink, and every node on a path other than the sink has exactly one
outgoing link in the plan.

A plan also lists **selected switches**, the combine ids, in ascending id
order. A selected switch must be reduction-capable and must be visited by at
least one path; a selected switch that no path visits is illegal, and the sink
is never selectable. A plan violating any rule in this section is **illegal**.

## 3. Links

Every payload leaving a node on the plan's outgoing link is transmitted on
that link. The link serves its payloads one at a time in the order of
`(ready tick, smallest origin id)`, both ascending. A payload with ready
tick `t` starts at `max(t, link_free)` where `link_free` is 0 before the
first transmission; it occupies the link for `ceil(payload_bytes / rate)`
ticks, after which the link is free for the next payload; it arrives at the
link's `to` node `latency` ticks after the occupation ends, and its ready tick
there is that arrival tick. The link's bytes are `payload_bytes` per payload
transmitted; a link no payload uses carries 0.

## 4. Switches and endpoints

At a node that is not selected, arriving payloads stay distinct and each is
forwarded on the node's outgoing link with its own ready tick.

At a selected switch, every payload reaching the node participates in one
reduction that happens after the last of them arrives. At least two payloads
must reach a selected switch; a selected switch reached by fewer than two is
illegal. The reduction takes zero ticks: it emits one payload of
`payload_bytes` whose origins are the union of the participants' origins,
whose value is defined by section 1, and whose ready tick is the latest
arrival. It reserves one accumulator of `payload_bytes` at that switch; the
reduction is legal only when `payload_bytes <= accumulator_capacity`. A
plan whose selected switch would exceed its capacity is illegal.

A plan's **total accumulator allocation** is `payload_bytes` times the
number of selected switches.

## 5. Sink

The sink is the final reducer and is not selectable. Every payload arriving
at the sink participates in one reduction defined exactly as in section 4,
with no capacity limit and no accumulator allocation. The plan's
**completion** is the arrival tick of the last payload to reach the sink; the
plan's **sink value** is the value of the resulting payload (equivalently,
the fold of all source values in ascending source id).

## 6. Totals

`total link bytes` is the sum of link bytes over all links. `link_bytes` per
link, `completion`, `total accumulator allocation`, `total link bytes` and
`sink value` are the observable outcome of a plan; two plans with the same
observables are still different plans when their paths or selected switches
differ.
