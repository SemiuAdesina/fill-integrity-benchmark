# Why the pruned search is exact (author-side)

The reference (`solution/fixed/src/recover.cpp`), oracle C (`research/bnb.py`),
oracle D (`oracle-b/oracle_d.py`) and solver_d (`adversarial/solver_d`) all
abandon a partial tree when a bound exceeds the completion the telemetry lists
(or the best completion found so far). The bound is the completion of the
partial tree, replayed for the sources routed so far, with every capable
switch that gathers two or more payloads selected, together with the unloaded
shortest cost of every source not yet routed. The search is exact only if this
is a lower bound on the completion of every legal plan that extends the
partial tree. The argument, under the public model (`fabric-replay-v1.md`):

**Order.** For two multisets of ticks X and Y say X ⊑ Y when, for every t,
the number of elements of X that are ≥ t is at most the number of elements
of Y that are ≥ t (equivalently: |X| ≤ |Y| and the i-th largest of X is at
most the i-th largest of Y for every i ≤ |X|). The completion of a plan is
the largest arrival tick at the sink, so X ⊑ Y implies max X ≤ max Y.

**Links preserve ⊑.** A link serves the payloads ready at its tail in
ascending ready tick (ties by origin id, which does not change the finish
multiset), each occupying the link for `occ` ticks and starting no earlier
than it is ready and no earlier than the link is free. With ρ(i) the i-th
largest ready tick among k payloads, the m-th largest finish is
max over i ≥ m of ρ(i) + (i − m + 1)·occ. If the ready multiset R' ⊑ R then
ρ'(i) ≤ ρ(i) for every i ≤ |R'| ≤ |R|, so every term is smaller and the
finish multiset satisfies F' ⊑ F. Adding the latency to every element
preserves ⊑.

**Nodes preserve ⊑.** At an unselected node the multiset forwarded is the
union of the arrivals, and unions of ⊑-related multisets are ⊑-related. At a
selected switch one payload leaves, ready at the largest arrival; if the
arrivals satisfy A' ⊑ A then max A' ≤ max A, so {max A'} ⊑ {max A}. A
switch that gathers one payload passes it through unchanged, which is the
same as selecting it.

**Two consequences.** (1) *Removing sources never raises completion.* Take a
legal plan P over all sources and the plan P' over a subset of sources whose
tree is P's tree restricted to their paths and whose selected switches are
P's restricted to that tree (a switch left with one payload passes through).
By induction from the leaves, at every node the multiset in P' is ⊑ the
multiset in P (a source's payload is ready at 0 in both, or absent in P'),
so completion(P') ≤ completion(P). (2) *Selecting a switch never raises
completion.* {max A} ⊑ A, so replacing "forward A" by "reduce A" at any
switch gives a ⊑-smaller multiset downstream and, by the same induction,
a completion that is no larger.

**The bound.** Let T be a partial tree for the routed sources S' and P any
legal plan over all sources that extends T. By (1), the restriction P|S' has
completion ≤ completion(P). P|S' selects some subset of the capable switches
on T; by (2) selecting every capable switch that gathers two or more payloads
gives a completion ≤ completion(P|S'). That replay is the first term of the
bound. Every source not yet routed still needs a path from its node to the
sink, on which it occupies each link for at least `occ` ticks and waits at
least `latency`, so its arrival, and hence completion(P), is at least the
unloaded shortest cost from its node. Together: bound(T) ≤ completion(P).

**Ties.** The search prunes only when the bound is *strictly* greater than
the cap. A partial tree whose bound equals the cap is kept, because the plan
that attains the cap may still win on allocation, total bytes or plan key.
A leaf enumerates every combine subset of the tree's capable switches and
compares the full objective explicitly, so no tie is decided by search order.

**Telemetry pruning.** A listed link that carried bytes must be on the tree:
if its tail node already leaves on another link, no extension can use it;
if no unrouted source can still reach its tail off the tree, no extension
can use it. For a listed link on the tree, the payloads crossing it are at
most one per source routed through it plus one per source still unrouted,
and, when its tail cannot reduce, at least one per branch entering the tail
plus the sources at the tail. A listed count outside those bounds admits no
extension. Links listed with 0 bytes are removed before the search: a plan
that used one would not reproduce the telemetry.

**Evidence.** `research/validate_cd.py`: on 2,400 random fabrics (1 to 4
sources, up to 3 capable switches, mixed telemetry, with and without
completion, deliberately infeasible perturbations; seeds 101–104, 201, 301),
exhaustive oracle A, oracles C and D, the reference and solver_d returned
the same status, plan and sink value in every case (solver_d was rebuilt
twice for speed; its final build ran seed 301 and every fixture). On every retained fixture, the certified answer
is the agreement of all four searches (`certification.json`).
