
# The scoring model

This document defines the objective exactly. The scorer that implements it ships
with the project at `src/scoring.rs` and is authoritative: it is frozen, it is
not part of what you are asked to write, and the graded run uses its own pristine
copy of it. Replacing it, wrapping it, or computing a score a different way
changes nothing about what is being measured.

Your task is the search, not the model.

## Inheritance states

A progeny receives two homologs from each parent. Which two is the *inheritance
state* for that parent at that marker, written as a pair `(a, b)` of homolog
indices.

The pair is **unordered**. Receiving homolog 1 and homolog 2 is the same event as
receiving homolog 2 and homolog 1; there is no first or second gamete slot. The
canonical written form always has `a <= b`, and the ten legal states for one
parent at one marker are, in this exact order:

```
index  0      1      2      3      4      5      6      7      8      9
state (0,0)  (0,1)  (0,2)  (0,3)  (1,1)  (1,2)  (1,3)  (2,2)  (2,3)  (3,3)
```

This ordering is the canonical state index and is used wherever a state must be
compared, sorted or serialised.

A state with `a == b` is the *double reduction* form: the same homolog is
received twice. It is a real biological event, not a degenerate encoding of a
missing value.

## Progeny dosage

Given inheritance states `(a, b)` for parent `A` and `(c, d)` for parent `B`, and
a parental haplotype assignment `hap`, the progeny's alternate allele dosage at
marker `m` is

```
dosage(m) = hap[A][a][m] + hap[A][b][m] + hap[B][c][m] + hap[B][d][m]
```

Note that when `a == b` the same homolog contributes twice. This is deliberate
and follows from receiving that homolog twice.

## The three contributions

The total score is a sum of three kinds of term. Every term is a natural
logarithm and every sum is taken over natural logarithms, so the total is a log
likelihood.

### 1. Emission

At every marker, for every progeny, add

```
child_gl[c][m][ dosage(m) ]
```

This is the only place the read evidence enters.

### 2. Double reduction

At every marker, for every progeny, **for each parent independently**, add

```
if state is (x, x):   ln( rate[P][m] )
otherwise:            ln( 1 - rate[P][m] )
```

The second branch is not optional. A marker where the progeny did not double
reduce still contributes `ln(1 - rate[P][m])`, and omitting it makes the double
reduction states look uniformly better than they are.

A state `(x, x)` at a marker where `allowed[P][m]` is `false` is **illegal**.
An illegal state has no score. A path containing one is not a worse path, it is
not a path at all, and any candidate reconstruction containing one is rejected
outright rather than scored.

### 3. Transition

Between consecutive markers `m` and `m + 1`, for every progeny, for each parent
independently, let `S = {a, b}` be the state at `m` and `S' = {a', b'}` the state
at `m + 1`, both treated as **multisets** of size two. Let

```
k = 2 - |S intersect S'|        (multiset intersection)
```

so `k` is the number of the two inherited homologs that changed, and `k` is `0`,
`1` or `2`. Add

```
k * ln( r ) + (2 - k) * ln( 1 - r )      where r = recombination[P][m]
```

The multiset intersection is what makes this well defined when a homolog is
repeated. For example `S = {1, 1}` and `S' = {1, 2}` intersect in the single
element `1`, giving `k = 1`, not `k = 2`. Treating the states as sets rather than
multisets gets this case wrong.

There is no transition term before the first marker. The first marker
contributes emission and double reduction only, and no prior distribution over
states is applied.

## The total

```
total = sum over progeny c
          sum over markers m
            emission(c, m)
          + dr(A, c, m) + dr(B, c, m)
          + sum over markers m in 0 .. M-1
              trans(A, c, m) + trans(B, c, m)
```

Summation order is fixed for reproducibility: progeny ascending, then marker
ascending, and within a marker the emission term, then parent `A`'s double
reduction term, then parent `B`'s, then parent `A`'s transition term into the
next marker, then parent `B`'s. The frozen scorer accumulates in exactly this
order, in `f64`. Accumulating in a different order can differ in the last bits,
which is why the comparison tolerance below exists.

## What is being maximised

Over all parental haplotype assignments consistent with `parent_dosage`, and all
legal inheritance state paths for all progeny, find the assignment and paths
that maximise `total`.

The maximum is **global**. A reconstruction that is optimal marker by marker, or
optimal for each progeny given a fixed parental guess, is generally not the
global maximum, and the graded instances are built so that it is not. Hill
climbing from a per marker dosage call, or accepting the first local optimum a
greedy pass reaches, produces a strictly lower total on these instances.

## Comparison tolerance

Two totals are considered equal when they differ by at most `1e-8` in absolute
value. This tolerance exists to absorb floating point reordering, and nothing
else. It is not slack in the objective: a reconstruction whose total is below the
optimum by more than `1e-8` is wrong.

Where this document says two reconstructions have "the same score", it means
equal within this tolerance.
