
# Worked example

`small.cross.json` and `small.result.json` are a matched pair: the second is the
global optimum for the first, in the canonical form `spec/result-format-v1.md`
requires, with the digest `spec/result-format-v1.md` defines.

The example is much smaller than the input at `/app/cross.json` — five markers
and three progeny — so it can be worked through by hand. It is here to settle
questions about the two document formats and about canonical form. It is not a
test of a search: an exhaustive enumeration solves it, and passing it says
nothing about whether a solver scales.

## What it exercises

| Property | Where to see it |
|---|---|
| Dosage constrains the allele columns | `parent_dosage.A` is `[3, 2, 3, 4, 1]`; marker 3 admits one column, marker 1 admits six |
| Double reduction legality is per parent and per marker | `double_reduction.*.allowed` is false at markers 1 and 2, true elsewhere, and `rate` is 0 exactly where it is false |
| Recombination couples adjacent markers | progeny 0 changes its parent B state between markers 1 and 2 |
| Homolog labels are arbitrary | parent A's rows 1 and 2 are identical, so several relabellings give the same reconstruction |

That last row is the one worth dwelling on. When two homolog rows are equal, the
relabellings that swap them leave the haplotypes untouched but rewrite the
inheritance states, so more than one state path describes the same biology.
Canonical form has to pick one of them, and `spec/result-format-v1.md` says
which.

## Using it

This five-marker illustration is intentionally below the accepted 12-marker
task floor, and the frozen input reader therefore rejects it as an executable
task input. Read `small.cross.json` beside `small.result.json` field by field to
check the formats and canonical ordering: the haplotype rows, every progeny's
two state paths, `total_log_likelihood` and `canonical_digest`.

A total that matches while the reconstruction differs means the answer is one of
several optima and not the required representative. That is a canonical form or
tie-breaking problem, not a search problem, and the specification is the place
to resolve it.
