# Deterministic fabric replay

Each source chooses a simple directed path to the sink. If selected paths meet,
they must share their entire suffix, so the selected links induce one tree.
The `source_paths` argument is aligned with sources in ascending source-id order.
Every source begins at tick zero with `(bytes,value,source-id)`.

At a node, child payloads arrive after their child link. A link serializes
payloads by `(ready tick, origin-id vector)`. A transfer finishes at
`max(ready,link-free) + latency + ceil(bytes/bytes_per_tick)`; link bytes are
the exact sum sent.

A selected combine consumes all payloads at that node after the last arrives.
There must be at least two, all must have equal byte size, and the switch must
be reduction-capable. Allocation is `payload-count * bytes` and is legal when
equal to the stated capacity. The output has one payload of the common byte
size, ready at the last arrival. Values are folded in canonical payload order
with the supplied signed saturating-add bounds after every operation. Origins
are concatenated and sorted. Without a combine, payloads remain distinct.

Sink completion is the latest arriving tick. Sink values are ordered by origin
vectors. Observed link bytes and completion must match exactly.

Recover the consistent plan minimizing: completion tick; total accumulator
allocation; total link bytes; then canonical encoding of sorted `(node,link)`
parents and sorted combine nodes.
