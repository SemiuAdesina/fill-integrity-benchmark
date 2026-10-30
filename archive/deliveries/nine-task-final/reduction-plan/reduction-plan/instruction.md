# Recover an in-network reduction plan

Repair `/app/src/recover.cpp`. The public API in `/app/include/fabric.hpp` and
the exact replay contract in `/app/spec/fabric-model.md` are frozen.

Given a topology, source shards, reduction-capable switches, accumulator limits,
partial link-byte observations and an optional completion observation, recover
the canonical globally optimal reduction tree, combine placement, link flows,
accumulator allocation, sink value and completion time. Candidate routes are
all bounded simple topology paths; a legal selection must induce one tree.

Use the supplied saturating reduction operation and exact event order. Capacity
equality is legal. Do not use greedy routing, host-only reduction, independent
link estimates, ordinary addition, fixture tables, or input/container order.
Everything builds offline. The candidate-agent limit is 7000 seconds.

The starter already contains the deterministic replay engine. Your repair must
implement the global recovery logic without changing the frozen public replay
contract. Run `/app/build/reduction_diagnostic` after building; its synthetic
public cases report which documented semantic family remains incorrect without
revealing verifier fixtures or hidden answers.

Public verification groups cover saturating boundaries and operation order,
combine-dependent traffic, uncombined serialization, exact accumulator
capacity, global observation recovery, suffix-consistent trees, canonical
equal routes, non-first global choices, ready-time scheduling, and a compound
capacity/symmetry/route/replay case. Fixtures and answers remain verifier-owned.
