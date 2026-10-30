
# Reference plan

How the reference solver reaches the global optimum, and why each step is
allowed to discard what it discards. Nothing here is a heuristic: every pruning
rule below is proved to leave at least one optimal assignment reachable.

## Shape of the problem

The objective in `spec/scoring-model.md` is a sum of three kinds of term:

- an emission term per progeny per marker, `child_gl[c][m][dosage]`, where the
  dosage is determined jointly by the parental allele columns at that marker and
  the progeny's two inheritance states there;
- a double reduction term per progeny per parent per marker, depending only on
  the state and the marker;
- a transition term per progeny per parent per interval, depending only on the
  two states and the interval.

Only the emission term depends on the parental alleles at all. That single fact
is what the search is built around.

Given the parental haplotypes, the progeny are conditionally independent, and
each one reduces to a Viterbi problem over `10 x 10 = 100` joint states per
marker. Given the progeny paths, the haplotypes would follow immediately. Neither
is given, and choosing them one at a time is exactly the marker-at-a-time error
the task warns about, so the search branches on the shared quantity — the
columns — and carries every progeny's dynamic programme forward together.

## Search space

`parent_dosage` fixes the number of alternate alleles at each marker, so a
parent has `C(4, dosage)` admissible allele columns there: one, four, six, four
or one. At most six per parent, so at most thirty-six joint column pairs per
marker.

Enumerating column assignments is `6^markers` per parent in the worst case, which
is around `2.2e9` at twelve markers and vastly more at twenty-eight. Brute force
is not available at any size in the declared range. Three things make the exact
answer reachable anyway.

### 1. Factorising the progeny transition

A progeny's joint state is a pair of per-parent states, and the transition term
factorises across parents. Advancing one progeny by one marker is therefore not a
`100 x 100` product but two `10 x 10` passes, one per parent, over an
intermediate. This is done once per node, before the node's children are
generated, because it does not depend on which columns those children pick.

### 2. Two admissible bounds

At a node, the markers up to and including the current one are decided and their
contribution is exact. Two independent relaxations bound everything after it.

`RemainingBound` frees the columns: for each progeny and each joint state, the
best score reachable to the end of the chromosome if the parental alleles were
chosen freely at every remaining marker, while the path is still required to be
continuous. Precomputed once per cross by a backward pass.

`MarkerBound` keeps the columns shared and frees the path instead: each remaining
interval is charged `2N(ln(1 - rA) + ln(1 - rB))`, the largest transition
contribution any path can collect there. This is an upper bound because `r <=
0.5` on every interval, which forces `ln(r) <= ln(1 - r)`, so no switch can pay
better than staying put.

Neither bound dominates the other — the first is tight when the paths are
constrained and the columns are not, the second when the reverse holds — and the
minimum of two upper bounds is an upper bound, so the search uses `min`.

### 3. Homolog symmetry reduction

Relabelling one parent's four homologs maps any assignment to an equally scoring
one. The search never explores two members of the same relabelling class: a
`RowOrder` bitmask records which orderings of the four rows are still consistent
with the columns chosen so far, and a column that would make the assignment
non-minimal within its class prunes the branch.

This is safe because the relation is an equivalence and the retained member is
the class minimum under the same row-major order the result format uses for
canonical form. It is not merely a speed-up applied afterwards: applying it
during the search is what keeps the tree from carrying twenty-four copies of
every subtree.

## Ties

Pruning uses a tolerance. A node is discarded only when its optimistic total is
strictly below `incumbent - 1e-9`, so a branch that can still tie survives. This
matters because the required answer among equal-scoring distinct optima is the
smallest under the row-major order, not whichever was found first. Discarding
equal-scoring branches would make the answer depend on the order of discovery.

When a complete assignment ties the incumbent, the incumbent is replaced only if
the new assignment is row-major smaller. Distinct column assignments always give
distinct haplotype matrices, so this is a total order and settles every tie.

Each progeny's path is then recovered forward, greedily and lexicographically
among states that attain the optimum, which is the recovery the result format
requires and is what makes repeated runs byte-identical.

## Files

```
solution/solve.sh                        thin launcher
solution/fixed/src/solver.rs             entry point and assembly
solution/fixed/src/solver/columns.rs     admissible columns, RowOrder symmetry
solution/fixed/src/solver/progeny_dp.rs  precomputed terms, factorised advance
solution/fixed/src/solver/bounds.rs      RemainingBound and MarkerBound
solution/fixed/src/solver/search.rs      branch and bound over columns
solution/fixed/src/solver/path.rs        forward path recovery
solution/fixed/src/solver/canonical.rs   canonical form and path remapping
```

`solve.sh` installs `fixed/src/solver.rs` and `fixed/src/solver/` over the
placeholder, builds offline in release, and runs the `phase` binary. It installs
nothing and fetches nothing. No frozen module is touched.

## What was checked during development

The exact search was cross-checked against exhaustive enumeration on small
crosses — every marker and offspring shape small enough to enumerate — and agreed
on the optimum in every case where enumeration completed. That is what
establishes the bounds are admissible; the argument above is why they should be,
and the cross-check is why we believe it.
